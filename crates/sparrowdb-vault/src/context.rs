//! Composed `context.jsonld` handling (§4.2, §4.3).
//!
//! Planned behaviour:
//! - load the root context; an array value composes further context documents
//!   by path relative to the referencing document, left-to-right;
//! - warn when a later context redefines a term/prefix with a *different*
//!   definition (identical re-declarations are silent);
//! - honour keyword aliases (`type` → `@type`, `id` → `@id`);
//! - read each folder context's `@base` — which stock JSON-LD processors
//!   ignore in referenced contexts, so this is Vault-LD's assembly rule, not
//!   JSON-LD's;
//! - never fetch remote contexts (spec §2.6: no runtime network access).

use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

use serde_json::Value;

use crate::error::VaultError;

/// One term definition after composition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TermDef {
    /// Expanded predicate IRI.
    pub iri: String,
    /// `@type` coercion: `Some("@id")` for IRI refs, an XSD IRI for literals.
    pub coerce: Option<String>,
    /// `@container: @set` or similar.
    pub container: Option<String>,
}

/// A term or prefix redefined with a conflicting definition (§4.2 SHOULD warn).
#[derive(Debug, Clone)]
pub struct ShadowWarning {
    pub term: String,
    pub first_defined_in: PathBuf,
    pub redefined_in: PathBuf,
}

/// The effective context for a vault, plus the scoped bases per folder.
#[derive(Debug, Clone, Default)]
pub struct ComposedContext {
    pub prefixes: BTreeMap<String, String>,
    pub terms: BTreeMap<String, TermDef>,
    /// Keyword aliases, e.g. `type` → `@type`.
    pub keyword_aliases: BTreeMap<String, String>,
    /// Folder (vault-relative) → `@base` declared by that folder's context.
    pub scoped_bases: BTreeMap<PathBuf, String>,
    pub warnings: Vec<ShadowWarning>,
}

#[derive(Clone, PartialEq, Eq)]
enum Def {
    Prefix(String),
    Alias(String),
    Term {
        iri: String,
        coerce: Option<String>,
        container: Option<String>,
    },
}

impl ComposedContext {
    /// Load and compose starting from `<vault>/context.jsonld`.
    pub fn load(vault_root: &Path) -> Result<Self, VaultError> {
        let root_ctx = vault_root.join("context.jsonld");
        if !root_ctx.is_file() {
            return Err(VaultError::MissingRootContext(vault_root.to_path_buf()));
        }
        let mut composed = ComposedContext::default();
        let mut origins: BTreeMap<String, (Def, PathBuf)> = BTreeMap::new();
        let mut stack = Vec::new();
        apply_document(
            vault_root,
            &root_ctx,
            &mut composed,
            &mut origins,
            &mut stack,
        )?;
        Ok(composed)
    }

    /// Governing `@base` for a note: nearest `context.jsonld` at or above the
    /// note's folder (§4.5).
    pub fn governing_base(&self, note_relative: &Path) -> Option<&str> {
        note_relative
            .ancestors()
            .skip(1)
            .find_map(|dir| self.scoped_bases.get(dir))
            .map(String::as_str)
    }
}

fn apply_document(
    vault_root: &Path,
    path: &Path,
    composed: &mut ComposedContext,
    origins: &mut BTreeMap<String, (Def, PathBuf)>,
    stack: &mut Vec<PathBuf>,
) -> Result<(), VaultError> {
    let canonical = normalize(path.to_path_buf());
    if stack.iter().any(|p| p == &canonical) {
        return Err(VaultError::ContextCycle(canonical));
    }
    stack.push(canonical.clone());

    let text = fs::read_to_string(path).map_err(|source| VaultError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let json: Value = serde_json::from_str(&text).map_err(|e| VaultError::MalformedJson {
        path: path.to_path_buf(),
        message: e.to_string(),
    })?;
    let ctx_value = json.get("@context").unwrap_or(&json);
    apply_value(vault_root, path, ctx_value, composed, origins, stack)?;
    stack.pop();
    Ok(())
}

fn apply_value(
    vault_root: &Path,
    from_file: &Path,
    value: &Value,
    composed: &mut ComposedContext,
    origins: &mut BTreeMap<String, (Def, PathBuf)>,
    stack: &mut Vec<PathBuf>,
) -> Result<(), VaultError> {
    match value {
        Value::Array(items) => {
            for item in items {
                apply_value(vault_root, from_file, item, composed, origins, stack)?;
            }
            Ok(())
        }
        Value::String(href) => apply_href(vault_root, from_file, href, composed, origins, stack),
        Value::Object(map) => apply_object(vault_root, from_file, map, composed, origins, stack),
        Value::Null => Ok(()),
        other => Err(VaultError::MalformedJson {
            path: from_file.to_path_buf(),
            message: format!("unsupported @context value: {other}"),
        }),
    }
}

fn apply_href(
    vault_root: &Path,
    from_file: &Path,
    href: &str,
    composed: &mut ComposedContext,
    origins: &mut BTreeMap<String, (Def, PathBuf)>,
    stack: &mut Vec<PathBuf>,
) -> Result<(), VaultError> {
    if is_remote(href) {
        return Err(VaultError::RemoteContext {
            from: from_file.to_path_buf(),
            href: href.to_string(),
        });
    }
    let target = resolve_href(from_file, href);
    if !target.is_file() {
        return Err(VaultError::Io {
            path: target,
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "context file not found"),
        });
    }
    apply_document(vault_root, &target, composed, origins, stack)
}

fn apply_object(
    vault_root: &Path,
    from_file: &Path,
    map: &serde_json::Map<String, Value>,
    composed: &mut ComposedContext,
    origins: &mut BTreeMap<String, (Def, PathBuf)>,
    stack: &mut Vec<PathBuf>,
) -> Result<(), VaultError> {
    if let Some(Value::String(base)) = map.get("@base") {
        let folder = folder_key(vault_root, from_file);
        composed.scoped_bases.insert(folder, base.clone());
    }
    if let Some(Value::String(href)) = map.get("@import") {
        apply_href(vault_root, from_file, href, composed, origins, stack)?;
    }

    for (key, value) in map {
        if key.starts_with('@') {
            continue;
        }
        match value {
            Value::String(s) => {
                if s.starts_with('@') {
                    record(composed, origins, from_file, key, Def::Alias(s.clone()));
                } else if s.ends_with('#') || s.ends_with('/') {
                    record(composed, origins, from_file, key, Def::Prefix(s.clone()));
                } else {
                    record(
                        composed,
                        origins,
                        from_file,
                        key,
                        Def::Term {
                            iri: s.clone(),
                            coerce: None,
                            container: None,
                        },
                    );
                }
            }
            Value::Object(def) => {
                let iri = def
                    .get("@id")
                    .and_then(Value::as_str)
                    .unwrap_or(key)
                    .to_string();
                let coerce = def.get("@type").and_then(Value::as_str).map(str::to_string);
                let container = container_of(def.get("@container"));
                record(
                    composed,
                    origins,
                    from_file,
                    key,
                    Def::Term {
                        iri,
                        coerce,
                        container,
                    },
                );
            }
            _ => {}
        }
    }
    Ok(())
}

fn record(
    composed: &mut ComposedContext,
    origins: &mut BTreeMap<String, (Def, PathBuf)>,
    from_file: &Path,
    key: &str,
    def: Def,
) {
    if let Some((prev, prev_path)) = origins.get(key) {
        if prev != &def {
            composed.warnings.push(ShadowWarning {
                term: key.to_string(),
                first_defined_in: prev_path.clone(),
                redefined_in: from_file.to_path_buf(),
            });
        }
    }
    origins.insert(key.to_string(), (def.clone(), from_file.to_path_buf()));
    match def {
        Def::Prefix(iri) => {
            composed.prefixes.insert(key.to_string(), iri);
        }
        Def::Alias(keyword) => {
            composed.keyword_aliases.insert(key.to_string(), keyword);
        }
        Def::Term {
            iri,
            coerce,
            container,
        } => {
            composed.terms.insert(
                key.to_string(),
                TermDef {
                    iri,
                    coerce,
                    container,
                },
            );
        }
    }
}

fn container_of(v: Option<&Value>) -> Option<String> {
    match v {
        Some(Value::String(s)) => Some(s.clone()),
        Some(Value::Array(items)) => items.first().and_then(Value::as_str).map(str::to_string),
        _ => None,
    }
}

fn is_remote(href: &str) -> bool {
    let h = href.trim();
    h.contains("://") || h.starts_with("//")
}

fn resolve_href(from_file: &Path, href: &str) -> PathBuf {
    let base = from_file.parent().unwrap_or(from_file);
    normalize(base.join(href))
}

fn folder_key(vault_root: &Path, context_file: &Path) -> PathBuf {
    let parent = context_file.parent().unwrap_or(context_file);
    match parent.strip_prefix(vault_root) {
        Ok(rel) if rel.as_os_str().is_empty() => PathBuf::from(""),
        Ok(rel) => rel.to_path_buf(),
        Err(_) => PathBuf::from(""),
    }
}

fn normalize(p: PathBuf) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn governing_base_picks_nearest_ancestor_context() {
        let mut ctx = ComposedContext::default();
        ctx.scoped_bases
            .insert(PathBuf::from(""), "https://example.org/".into());
        ctx.scoped_bases.insert(
            PathBuf::from("Ontologies/Culinary"),
            "https://example.org/culinary#".into(),
        );

        assert_eq!(
            ctx.governing_base(Path::new("Ontologies/Culinary/Classes/Recipe.md")),
            Some("https://example.org/culinary#")
        );
        assert_eq!(
            ctx.governing_base(Path::new("Recipes/hummus.md")),
            Some("https://example.org/")
        );
    }

    #[test]
    fn load_composes_array_and_collects_scoped_bases() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("Ontologies/Gadgets")).unwrap();
        fs::write(
            dir.path().join("context.jsonld"),
            r#"{
              "@context": [
                {
                  "@base": "https://example.org/",
                  "type": "@type",
                  "id": "@id",
                  "owl": "http://www.w3.org/2002/07/owl#",
                  "label": "rdfs:label"
                },
                "Ontologies/Gadgets/context.jsonld"
              ]
            }"#,
        )
        .unwrap();
        fs::write(
            dir.path().join("Ontologies/Gadgets/context.jsonld"),
            r#"{
              "@context": {
                "@base": "https://example.org/gadgets#",
                "owl": "http://www.w3.org/2002/07/owl#",
                "massGrams": { "@id": "https://example.org/gadgets#massGrams", "@type": "xsd:integer" }
              }
            }"#,
        )
        .unwrap();

        let ctx = ComposedContext::load(dir.path()).unwrap();
        assert_eq!(
            ctx.keyword_aliases.get("type").map(String::as_str),
            Some("@type")
        );
        assert_eq!(
            ctx.prefixes.get("owl").map(String::as_str),
            Some("http://www.w3.org/2002/07/owl#")
        );
        assert_eq!(
            ctx.scoped_bases.get(Path::new("")).map(String::as_str),
            Some("https://example.org/")
        );
        assert_eq!(
            ctx.scoped_bases
                .get(Path::new("Ontologies/Gadgets"))
                .map(String::as_str),
            Some("https://example.org/gadgets#")
        );
        assert_eq!(
            ctx.terms.get("massGrams").map(|t| t.iri.as_str()),
            Some("https://example.org/gadgets#massGrams")
        );
        // Identical re-declaration of owl: is silent.
        assert!(ctx.warnings.is_empty());
    }

    #[test]
    fn conflicting_redefinition_warns() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("Ontologies/A")).unwrap();
        fs::write(
            dir.path().join("context.jsonld"),
            r#"{
              "@context": [
                { "label": "rdfs:label" },
                "Ontologies/A/context.jsonld"
              ]
            }"#,
        )
        .unwrap();
        fs::write(
            dir.path().join("Ontologies/A/context.jsonld"),
            r#"{ "@context": { "label": "skos:prefLabel" } }"#,
        )
        .unwrap();

        let ctx = ComposedContext::load(dir.path()).unwrap();
        assert_eq!(ctx.warnings.len(), 1);
        assert_eq!(ctx.warnings[0].term, "label");
        assert_eq!(
            ctx.terms.get("label").map(|t| t.iri.as_str()),
            Some("skos:prefLabel")
        );
    }

    #[test]
    fn remote_context_ref_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("context.jsonld"),
            r#"{ "@context": ["https://example.org/context.jsonld"] }"#,
        )
        .unwrap();
        let err = ComposedContext::load(dir.path()).unwrap_err();
        assert!(matches!(err, VaultError::RemoteContext { .. }));
    }
}
