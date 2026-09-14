//! `check`: dry-run lint of a whole vault.
//!
//! This slice is DB-free. `UnknownProperty` / `RelationRangeViolation` need the
//! ontology and land with the sync slice; they are never emitted here.
//!
//! Errors match the actionable style of `create_entity`
//! ("Unknown property 'typo_field'. Valid: [...]"), carry file + line-adjacent
//! context, and a fix suggestion. Any error ⇒ nonzero exit from the CLI.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::context::ComposedContext;
use crate::error::VaultError;
use crate::identity::is_conforming_explicit_id;
use crate::layout::{NotePath, Vault};
use crate::parse::{split_frontmatter, Frontmatter, FrontmatterValue, WikiLink};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticKind {
    /// Frontmatter is not valid YAML; the file is skipped, the run continues.
    MalformedFrontmatter,
    /// Key not resolvable through the composed context (host keys excepted).
    UnknownProperty,
    /// Relation target's class violates the relation's range.
    RelationRangeViolation,
    /// `[[Link]]` names no participating note (§4.4.1 MUST flag).
    DanglingWikiLink,
    /// Two participating notes share a file name without an explicit `id`.
    AmbiguousIdentity,
    /// `id` present but not an absolute IRI (§4.5).
    RelativeExplicitId,
    /// A note carries its own `@context` (§4.2 MUST NOT).
    InlineContext,
    /// Prefixed key such as `rdfs:comment:` (§4.3).
    PrefixedFieldName,
}

#[derive(Debug, Clone, Serialize)]
pub struct Diagnostic {
    pub file: PathBuf,
    /// 1-based line in the file, when the parser can attribute it.
    pub line: Option<usize>,
    pub field: Option<String>,
    pub kind: DiagnosticKind,
    pub message: String,
    pub suggestion: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct CheckReport {
    pub files_checked: usize,
    pub errors: Vec<Diagnostic>,
    pub warnings: Vec<Diagnostic>,
}

impl CheckReport {
    pub fn is_clean(&self) -> bool {
        self.errors.is_empty()
    }
}

/// DB-free vault lints. Ontology-dependent kinds are skipped until sync.
pub fn check(vault: &Vault) -> Result<CheckReport, VaultError> {
    let ctx = ComposedContext::load(&vault.root)?;
    let notes = vault.notes()?;

    let mut report = CheckReport::default();
    let mut parsed: Vec<ParsedNote> = Vec::new();

    for note in &notes {
        report.files_checked += 1;
        let abs = vault.root.join(&note.relative);
        let text = fs::read_to_string(&abs).map_err(|source| VaultError::Io {
            path: abs.clone(),
            source,
        })?;
        let raw = split_frontmatter(&text);
        let Some(yaml) = raw.frontmatter else {
            continue;
        };
        match Frontmatter::decode(note.clone(), yaml) {
            Ok(fm) => parsed.push(ParsedNote {
                note: note.clone(),
                fm,
                yaml_line: raw.frontmatter_line,
            }),
            Err(VaultError::MalformedYaml { line, message, .. }) => {
                report.errors.push(Diagnostic {
                    file: note.relative.clone(),
                    line: line.map(|l| raw.frontmatter_line.saturating_add(l.saturating_sub(1))),
                    field: None,
                    kind: DiagnosticKind::MalformedFrontmatter,
                    message,
                    suggestion: Some("Fix the YAML syntax in the frontmatter block".into()),
                });
            }
            Err(e) => return Err(e),
        }
    }

    let type_keys = keyword_keys(&ctx, "@type");
    let id_keys = keyword_keys(&ctx, "@id");

    let mut by_name: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    let mut participating: Vec<bool> = Vec::with_capacity(parsed.len());

    for (idx, item) in parsed.iter().enumerate() {
        lint_note_local(&item.fm, &item.note.relative, &mut report);
        let has_type = type_keys.iter().any(|k| item.fm.fields.contains_key(k));
        participating.push(has_type);
        if has_type {
            if let Some(name) = item.note.note_name() {
                by_name.entry(name.to_string()).or_default().push(idx);
            }
        }
    }

    for (name, idxs) in &by_name {
        if idxs.len() > 1 {
            for &idx in idxs {
                report.warnings.push(Diagnostic {
                    file: parsed[idx].note.relative.clone(),
                    line: None,
                    field: Some("id".into()),
                    kind: DiagnosticKind::AmbiguousIdentity,
                    message: format!(
                        "multiple participating notes are named `{name}`; bare [[wiki links]] to that name are ambiguous"
                    ),
                    suggestion: Some(
                        "Declare an absolute id on at least one note and path-qualify links"
                            .into(),
                    ),
                });
            }
        }
    }

    let mut name_index: BTreeMap<String, Vec<&NotePath>> = BTreeMap::new();
    for (item, is_part) in parsed.iter().zip(participating.iter()) {
        if !*is_part {
            continue;
        }
        if let Some(name) = item.note.note_name() {
            name_index
                .entry(name.to_string())
                .or_default()
                .push(&item.note);
        }
    }

    for (item, is_part) in parsed.iter().zip(participating.iter()) {
        if !*is_part {
            continue;
        }
        if let Some(id) = first_field(&item.fm, &id_keys) {
            let as_str = value_as_str(id);
            if !as_str.is_some_and(is_conforming_explicit_id) {
                let shown = match as_str {
                    Some(s) => s.to_string(),
                    None => match id {
                        FrontmatterValue::Integer(n) => n.to_string(),
                        FrontmatterValue::Float(n) => n.to_string(),
                        FrontmatterValue::Bool(b) => b.to_string(),
                        FrontmatterValue::Null => "null".into(),
                        _ => "non-string".into(),
                    },
                };
                report.errors.push(Diagnostic {
                    file: item.note.relative.clone(),
                    line: Some(item.yaml_line),
                    field: Some("id".into()),
                    kind: DiagnosticKind::RelativeExplicitId,
                    message: format!("explicit id `{shown}` is not an absolute http(s) IRI"),
                    suggestion: Some("Use a full IRI, e.g. https://example.org/name".into()),
                });
            }
        }

        let mut links = Vec::new();
        for (field, value) in &item.fm.fields {
            collect_links(value, field, &mut links);
        }
        for (field, link) in links {
            if !link_resolves(link, &name_index) {
                report.errors.push(Diagnostic {
                    file: item.note.relative.clone(),
                    line: Some(item.yaml_line),
                    field: Some(field.clone()),
                    kind: DiagnosticKind::DanglingWikiLink,
                    message: format!("[[{}]] names no participating note", display_link(link)),
                    suggestion: Some(
                        "Create the target note with typed frontmatter, or fix the link name"
                            .into(),
                    ),
                });
            }
        }
    }

    Ok(report)
}

struct ParsedNote {
    note: NotePath,
    fm: Frontmatter,
    yaml_line: usize,
}

fn keyword_keys(ctx: &ComposedContext, keyword: &str) -> Vec<String> {
    let mut keys = vec![keyword.to_string()];
    for (alias, target) in &ctx.keyword_aliases {
        if target == keyword {
            keys.push(alias.clone());
        }
    }
    keys
}

fn first_field<'a>(fm: &'a Frontmatter, keys: &[String]) -> Option<&'a FrontmatterValue> {
    keys.iter().find_map(|k| fm.fields.get(k))
}

fn lint_note_local(fm: &Frontmatter, file: &Path, report: &mut CheckReport) {
    for key in fm.fields.keys() {
        if key == "@context" {
            report.errors.push(Diagnostic {
                file: file.to_path_buf(),
                line: None,
                field: Some(key.clone()),
                kind: DiagnosticKind::InlineContext,
                message: "notes must not carry their own @context (spec §4.2)".into(),
                suggestion: Some("Move term definitions into context.jsonld".into()),
            });
        }
        if key.contains(':') && !key.starts_with('@') {
            report.errors.push(Diagnostic {
                file: file.to_path_buf(),
                line: None,
                field: Some(key.clone()),
                kind: DiagnosticKind::PrefixedFieldName,
                message: format!("field name `{key}` is prefixed; keys must be the short alias"),
                suggestion: Some(
                    "Write the short context alias (e.g. comment: not rdfs:comment:)".into(),
                ),
            });
        }
    }
}

fn collect_links<'a>(
    value: &'a FrontmatterValue,
    field: &str,
    out: &mut Vec<(String, &'a WikiLink)>,
) {
    match value {
        FrontmatterValue::Link(link) => out.push((field.to_string(), link)),
        FrontmatterValue::List(items) => {
            for item in items {
                collect_links(item, field, out);
            }
        }
        FrontmatterValue::Mapping(map) => {
            for (k, v) in map {
                collect_links(v, k, out);
            }
        }
        _ => {}
    }
}

fn link_resolves(link: &WikiLink, by_name: &BTreeMap<String, Vec<&NotePath>>) -> bool {
    let Some(cands) = by_name.get(&link.name) else {
        return false;
    };
    // Spec §4.4.1: path disambiguates only when several participating notes
    // share a name; resolution uses the final segment.
    if cands.len() == 1 {
        return true;
    }
    match &link.path {
        Some(path) => cands.iter().any(|n| note_matches_path(n, path, &link.name)),
        None => true,
    }
}

fn note_matches_path(note: &NotePath, path: &str, name: &str) -> bool {
    let rel = note.relative.to_string_lossy().replace('\\', "/");
    let want = format!("{path}/{name}.md");
    rel == want || rel.ends_with(&format!("/{want}"))
}

fn display_link(link: &WikiLink) -> String {
    match &link.path {
        Some(p) => format!("{p}/{}", link.name),
        None => link.name.clone(),
    }
}

fn value_as_str(v: &FrontmatterValue) -> Option<&str> {
    match v {
        FrontmatterValue::String(s) | FrontmatterValue::Curie(s) => Some(s),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write_min_context(root: &Path) {
        fs::write(
            root.join("context.jsonld"),
            r#"{
              "@context": {
                "@base": "https://example.org/",
                "type": "@type",
                "id": "@id"
              }
            }"#,
        )
        .unwrap();
    }

    #[test]
    fn flags_dangling_relative_malformed_inline_and_prefixed() {
        let dir = tempfile::tempdir().unwrap();
        write_min_context(dir.path());
        fs::create_dir_all(dir.path().join("Items")).unwrap();
        fs::write(
            dir.path().join("Items/widget.md"),
            "---\ntype: owl:Class\nlabel: Widget\n---\n# Widget\n",
        )
        .unwrap();
        fs::write(
            dir.path().join("Items/sprocket.md"),
            "---\ntype: \"[[Widget]]\"\nrelated: \"[[NoSuch]]\"\nid: sprocket\nrdfs:comment: nope\n\"@context\": { \"x\": \"y\" }\n---\n",
        )
        .unwrap();
        fs::write(dir.path().join("Items/broken.md"), "---\ntype: [\n---\n").unwrap();

        let vault = Vault::open(dir.path()).unwrap();
        let report = check(&vault).unwrap();
        let kinds: Vec<_> = report.errors.iter().map(|d| d.kind.clone()).collect();
        assert!(
            kinds.contains(&DiagnosticKind::DanglingWikiLink),
            "{kinds:?}"
        );
        assert!(kinds.contains(&DiagnosticKind::RelativeExplicitId));
        assert!(kinds.contains(&DiagnosticKind::MalformedFrontmatter));
        assert!(kinds.contains(&DiagnosticKind::PrefixedFieldName));
        assert!(kinds.contains(&DiagnosticKind::InlineContext));
        assert!(!report.is_clean());
        assert!(report.files_checked >= 3);
    }

    #[test]
    fn clean_typed_notes_pass() {
        let dir = tempfile::tempdir().unwrap();
        write_min_context(dir.path());
        fs::create_dir_all(dir.path().join("Items")).unwrap();
        fs::write(
            dir.path().join("Items/Widget.md"),
            "---\ntype: owl:Class\n---\n",
        )
        .unwrap();
        fs::write(
            dir.path().join("Items/sprocket.md"),
            "---\ntype: \"[[Widget]]\"\n---\n",
        )
        .unwrap();
        let vault = Vault::open(dir.path()).unwrap();
        let report = check(&vault).unwrap();
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    }

    #[test]
    fn unique_name_resolves_regardless_of_path() {
        let dir = tempfile::tempdir().unwrap();
        write_min_context(dir.path());
        fs::create_dir_all(dir.path().join("Ontologies/Culinary/Classes")).unwrap();
        fs::create_dir_all(dir.path().join("Items")).unwrap();
        fs::write(
            dir.path().join("Ontologies/Culinary/Classes/Recipe.md"),
            "---\ntype: owl:Class\n---\n",
        )
        .unwrap();
        fs::write(
            dir.path().join("Items/Widget.md"),
            "---\ntype: owl:Class\n---\n",
        )
        .unwrap();
        fs::write(
            dir.path().join("Items/sprocket.md"),
            "---\ntype: \"[[Culinary/Recipe]]\"\nrelated: \"[[Nope/Widget]]\"\n---\n",
        )
        .unwrap();
        let vault = Vault::open(dir.path()).unwrap();
        let report = check(&vault).unwrap();
        let dangling: Vec<_> = report
            .errors
            .iter()
            .filter(|d| d.kind == DiagnosticKind::DanglingWikiLink)
            .collect();
        assert!(
            dangling.is_empty(),
            "unique names must resolve even with a wrong/partial path: {dangling:?}"
        );
    }

    #[test]
    fn integer_explicit_id_is_relative() {
        let dir = tempfile::tempdir().unwrap();
        write_min_context(dir.path());
        fs::create_dir_all(dir.path().join("Items")).unwrap();
        fs::write(
            dir.path().join("Items/Widget.md"),
            "---\ntype: owl:Class\n---\n",
        )
        .unwrap();
        fs::write(
            dir.path().join("Items/sprocket.md"),
            "---\ntype: \"[[Widget]]\"\nid: 123\n---\n",
        )
        .unwrap();
        let vault = Vault::open(dir.path()).unwrap();
        let report = check(&vault).unwrap();
        assert!(
            report
                .errors
                .iter()
                .any(|d| d.kind == DiagnosticKind::RelativeExplicitId),
            "{:?}",
            report.errors
        );
    }
}
