//! Note parsing: frontmatter split (§4.1) and wiki-link grammar (§4.4.1).
//!
//! Only the dependency-free primitives are implemented in the scaffold. YAML
//! decoding of the frontmatter block lands in the parse slice, together with
//! the YAML crate choice (see README open questions).

use std::collections::BTreeMap;

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
    List(Vec<FrontmatterValue>),
}

/// Decoded frontmatter for one note.
#[derive(Debug, Clone, PartialEq)]
pub struct Frontmatter {
    pub note: NotePath,
    pub fields: BTreeMap<String, FrontmatterValue>,
}

impl Frontmatter {
    /// Decode a raw YAML block. Bad YAML is reported per file and skipped by
    /// `sync`; it must never abort the rest of the vault.
    pub fn decode(note: NotePath, yaml: &str) -> Result<Self, VaultError> {
        let _ = (note, yaml);
        Err(VaultError::not_implemented("Frontmatter::decode", "parse"))
    }
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
}
