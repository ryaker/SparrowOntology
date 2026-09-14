//! Note parsing: frontmatter split (§4.1), YAML decode, wiki-link grammar (§4.4.1).
//!
//! YAML decoding uses `saphyr` (YAML 1.2) with `MarkedYaml` spans so malformed
//! frontmatter can be reported with a line-adjacent location. See the crate
//! README for the YAML 1.1 (PyYAML) isomorphism tradeoff.

use std::collections::BTreeMap;

use saphyr::{LoadableYamlNode, MarkedYaml, Scalar, YamlData};

use crate::error::VaultError;
use crate::layout::NotePath;

/// A note split into its raw frontmatter block and body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawNote<'a> {
    /// YAML text between the `---` fences, without the fences.
    pub frontmatter: Option<&'a str>,
    /// 1-based line number where the frontmatter YAML starts, for diagnostics.
    pub frontmatter_line: usize,
    /// Everything after the closing fence (or the whole file if none).
    pub body: &'a str,
}

/// Split a note at its leading `---` fenced frontmatter block.
///
/// A file with no opening fence, or an unterminated one, has no frontmatter
/// and therefore does not participate in the graph (§4.4.1) — it is not an
/// error.
pub fn split_frontmatter(text: &str) -> RawNote<'_> {
    let no_fm = RawNote {
        frontmatter: None,
        frontmatter_line: 0,
        body: text,
    };
    let text_no_bom = text.strip_prefix('\u{feff}').unwrap_or(text);
    let Some(after_open) = strip_fence_line(text_no_bom) else {
        return no_fm;
    };

    let mut offset = 0;
    for line in after_open.split_inclusive('\n') {
        if line.trim_end_matches(['\r', '\n']) == "---" {
            let fm = &after_open[..offset];
            let body = &after_open[offset + line.len()..];
            return RawNote {
                frontmatter: Some(fm),
                frontmatter_line: 2,
                body,
            };
        }
        offset += line.len();
    }
    no_fm
}

fn strip_fence_line(s: &str) -> Option<&str> {
    s.strip_prefix("---\r\n")
        .or_else(|| s.strip_prefix("---\n"))
}

/// A frontmatter value after YAML decoding, before context resolution.
#[derive(Debug, Clone, PartialEq)]
pub enum FrontmatterValue {
    Link(WikiLink),
    /// Prefixed CURIE such as `sdo:Recipe` or `owl:Class`.
    Curie(String),
    String(String),
    Integer(i64),
    Float(f64),
    Bool(bool),
    Null,
    List(Vec<FrontmatterValue>),
    /// Nested mapping (e.g. an inline `@context` block, which `check` flags).
    Mapping(BTreeMap<String, FrontmatterValue>),
}

/// Decoded frontmatter for one note.
#[derive(Debug, Clone, PartialEq)]
pub struct Frontmatter {
    pub note: NotePath,
    pub fields: BTreeMap<String, FrontmatterValue>,
}

impl Frontmatter {
    /// Decode a raw YAML block. Bad YAML is reported per file and skipped by
    /// `sync`/`check`; it must never abort the rest of the vault.
    pub fn decode(note: NotePath, yaml: &str) -> Result<Self, VaultError> {
        let path = note.relative.clone();
        if yaml.trim().is_empty() {
            return Ok(Frontmatter {
                note,
                fields: BTreeMap::new(),
            });
        }

        let docs = MarkedYaml::load_from_str(yaml).map_err(|e| VaultError::MalformedYaml {
            path: path.clone(),
            line: Some(e.marker().line()),
            message: e.info().to_string(),
        })?;

        let Some(doc) = docs.into_iter().next() else {
            return Ok(Frontmatter {
                note,
                fields: BTreeMap::new(),
            });
        };

        let mapping = match yaml_as_mapping(&doc) {
            Ok(m) => m,
            Err(None) => {
                return Ok(Frontmatter {
                    note,
                    fields: BTreeMap::new(),
                });
            }
            Err(Some((line, message))) => {
                return Err(VaultError::MalformedYaml {
                    path,
                    line,
                    message,
                });
            }
        };

        let mut fields = BTreeMap::new();
        for (key_node, value_node) in mapping {
            let key = match mapping_key(key_node) {
                Some(k) => k,
                None => {
                    return Err(VaultError::MalformedYaml {
                        path,
                        line: Some(key_node.span.start.line()),
                        message: "frontmatter keys must be scalars".into(),
                    });
                }
            };
            match node_to_value(value_node) {
                Ok(v) => {
                    fields.insert(key, v);
                }
                Err((line, message)) => {
                    return Err(VaultError::MalformedYaml {
                        path,
                        line,
                        message,
                    });
                }
            }
        }

        Ok(Frontmatter { note, fields })
    }
}

fn yaml_as_mapping<'a>(
    doc: &'a MarkedYaml<'a>,
) -> Result<&'a saphyr::AnnotatedMapping<'a, MarkedYaml<'a>>, Option<(Option<usize>, String)>> {
    match &doc.data {
        YamlData::Mapping(map) => Ok(map),
        YamlData::Value(Scalar::Null) => Err(None),
        YamlData::Tagged(_, inner) => yaml_as_mapping(inner),
        _ => Err(Some((
            Some(doc.span.start.line()),
            "frontmatter must be a YAML mapping".into(),
        ))),
    }
}

fn mapping_key(node: &MarkedYaml<'_>) -> Option<String> {
    match &node.data {
        YamlData::Value(Scalar::String(s)) => Some(s.to_string()),
        YamlData::Value(Scalar::Integer(i)) => Some(i.to_string()),
        YamlData::Value(Scalar::Boolean(b)) => Some(b.to_string()),
        YamlData::Value(Scalar::FloatingPoint(f)) => Some(f.0.to_string()),
        YamlData::Value(Scalar::Null) => Some("null".into()),
        YamlData::Tagged(_, inner) => mapping_key(inner),
        _ => None,
    }
}

fn node_to_value(node: &MarkedYaml<'_>) -> Result<FrontmatterValue, (Option<usize>, String)> {
    match &node.data {
        YamlData::Value(Scalar::Null) => Ok(FrontmatterValue::Null),
        YamlData::Value(Scalar::Boolean(b)) => Ok(FrontmatterValue::Bool(*b)),
        YamlData::Value(Scalar::Integer(i)) => Ok(FrontmatterValue::Integer(*i)),
        YamlData::Value(Scalar::FloatingPoint(f)) => Ok(FrontmatterValue::Float(f.0)),
        YamlData::Value(Scalar::String(s)) => Ok(classify_string(s)),
        YamlData::Sequence(seq) => {
            let mut items = Vec::with_capacity(seq.len());
            for item in seq {
                items.push(node_to_value(item)?);
            }
            Ok(FrontmatterValue::List(items))
        }
        YamlData::Mapping(map) => {
            let mut fields = BTreeMap::new();
            for (k, v) in map {
                let key = mapping_key(k).ok_or_else(|| {
                    (
                        Some(k.span.start.line()),
                        "nested mapping keys must be scalars".to_string(),
                    )
                })?;
                fields.insert(key, node_to_value(v)?);
            }
            Ok(FrontmatterValue::Mapping(fields))
        }
        YamlData::Tagged(_, inner) => node_to_value(inner),
        YamlData::Alias(_) => Err((
            Some(node.span.start.line()),
            "YAML aliases are not supported in frontmatter".into(),
        )),
        YamlData::BadValue => Err((Some(node.span.start.line()), "invalid YAML scalar".into())),
        YamlData::Representation(raw, _, _) => Ok(classify_string(raw)),
    }
}

fn classify_string(s: &str) -> FrontmatterValue {
    if let Some(link) = WikiLink::parse(s) {
        return FrontmatterValue::Link(link);
    }
    if looks_like_curie(s) {
        return FrontmatterValue::Curie(s.to_string());
    }
    FrontmatterValue::String(s.to_string())
}

/// Compact IRI (`prefix:local`) as used in values (`owl:Class`, `sdo:Recipe`).
/// Absolute IRIs and host-tool strings with a colon are left as plain strings.
fn looks_like_curie(s: &str) -> bool {
    let s = s.trim();
    if s.starts_with("http://") || s.starts_with("https://") || s.starts_with("urn:") {
        return false;
    }
    let Some((prefix, local)) = s.split_once(':') else {
        return false;
    };
    !prefix.is_empty()
        && !local.is_empty()
        && !local.starts_with("//")
        && !prefix.contains(['/', ' ', '\t'])
        && !local.contains([' ', '\t'])
        && !local.contains(':')
}

/// `[[path/to/name#Fragment|alias]]` (§4.4.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WikiLink {
    /// Optional disambiguating path, e.g. `Recipes/Soups` (no trailing slash).
    pub path: Option<String>,
    /// Final segment — the note name used for resolution.
    pub name: String,
    /// Discarded for resolution; a tool MAY warn.
    pub fragment: Option<String>,
    /// Display only; MUST be ignored for resolution.
    pub alias: Option<String>,
}

impl WikiLink {
    /// Parse a single frontmatter string value. Returns `None` unless the whole
    /// value (after trimming) is exactly one `[[…]]` link.
    pub fn parse(value: &str) -> Option<Self> {
        let inner = value.trim().strip_prefix("[[")?.strip_suffix("]]")?;
        if inner.contains("[[") || inner.contains("]]") {
            return None;
        }

        let (target, alias) = match inner.split_once('|') {
            Some((t, a)) => (t, Some(a.to_string())),
            None => (inner, None),
        };
        let (target, fragment) = match target.split_once('#') {
            Some((t, f)) => (t, Some(f.to_string())),
            None => (target, None),
        };
        let (path, name) = match target.rsplit_once('/') {
            Some((p, n)) => (Some(p.to_string()), n),
            None => (None, target),
        };
        let name = name.trim();
        if name.is_empty() {
            return None;
        }
        Some(WikiLink {
            path,
            name: name.to_string(),
            fragment,
            alias,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_hummus_note() {
        let text = "---\ntype: \"[[Recipe]]\"\nprepTimeMinutes: 25\n---\n\n# Hummus\nA purée.\n";
        let raw = split_frontmatter(text);
        assert_eq!(
            raw.frontmatter,
            Some("type: \"[[Recipe]]\"\nprepTimeMinutes: 25\n")
        );
        assert_eq!(raw.body, "\n# Hummus\nA purée.\n");
    }

    #[test]
    fn no_or_unterminated_fence_means_no_frontmatter() {
        assert_eq!(split_frontmatter("# Just prose\n").frontmatter, None);
        assert_eq!(split_frontmatter("---\ntype: x\n").frontmatter, None);
    }

    #[test]
    fn handles_crlf_and_bom() {
        let raw = split_frontmatter("\u{feff}---\r\nlabel: X\r\n---\r\nbody");
        assert_eq!(raw.frontmatter, Some("label: X\r\n"));
        assert_eq!(raw.body, "body");
    }

    #[test]
    fn wiki_link_grammar() {
        assert_eq!(
            WikiLink::parse("[[Recipes/Soups/red-lentil-soup#Method|Soup]]"),
            Some(WikiLink {
                path: Some("Recipes/Soups".into()),
                name: "red-lentil-soup".into(),
                fragment: Some("Method".into()),
                alias: Some("Soup".into()),
            })
        );
        assert_eq!(WikiLink::parse("[[Chickpeas]]").unwrap().name, "Chickpeas");
        assert_eq!(WikiLink::parse("sdo:Recipe"), None);
        assert_eq!(WikiLink::parse("[[]]"), None);
        assert_eq!(WikiLink::parse("[[A]] and [[B]]"), None);
    }

    fn decode(yaml: &str) -> Frontmatter {
        Frontmatter::decode(
            NotePath {
                relative: "note.md".into(),
            },
            yaml,
        )
        .unwrap()
    }

    #[test]
    fn decode_hummus_shaped_frontmatter() {
        let fm = decode("type: \"[[Recipe]]\"\nprepTimeMinutes: 25\n");
        assert_eq!(
            fm.fields.get("type"),
            Some(&FrontmatterValue::Link(WikiLink {
                path: None,
                name: "Recipe".into(),
                fragment: None,
                alias: None,
            }))
        );
        assert_eq!(
            fm.fields.get("prepTimeMinutes"),
            Some(&FrontmatterValue::Integer(25))
        );
    }

    #[test]
    fn decode_recognizes_curie_and_bool_and_list() {
        let fm = decode("type: owl:Class\nflag: true\nparents: [\"[[A]]\", sdo:Recipe]\n");
        assert_eq!(
            fm.fields.get("type"),
            Some(&FrontmatterValue::Curie("owl:Class".into()))
        );
        assert_eq!(fm.fields.get("flag"), Some(&FrontmatterValue::Bool(true)));
        match fm.fields.get("parents") {
            Some(FrontmatterValue::List(items)) => {
                assert!(matches!(&items[0], FrontmatterValue::Link(_)));
                assert_eq!(items[1], FrontmatterValue::Curie("sdo:Recipe".into()));
            }
            other => panic!("expected list, got {other:?}"),
        }
    }

    #[test]
    fn yaml12_yes_is_a_string_not_a_bool() {
        // Documents the saphyr/YAML 1.2 choice vs PyYAML 1.1 (`yes` → true).
        let fm = decode("ok: yes\n");
        assert_eq!(
            fm.fields.get("ok"),
            Some(&FrontmatterValue::String("yes".into()))
        );
    }

    #[test]
    fn malformed_yaml_is_an_error_with_a_line() {
        let err = Frontmatter::decode(
            NotePath {
                relative: "broken.md".into(),
            },
            "type: [\n",
        )
        .unwrap_err();
        match err {
            VaultError::MalformedYaml { path, line, .. } => {
                assert_eq!(path.as_os_str(), "broken.md");
                assert!(line.is_some());
            }
            other => panic!("expected MalformedYaml, got {other:?}"),
        }
    }
}
