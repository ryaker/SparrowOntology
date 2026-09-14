//! Identity minting (§4.5).
//!
//! IRI = governing `@base` + percent-encoded file name without `.md`
//! (RFC 3987), unless the note declares an absolute `id`, used verbatim.
//! Folders never enter the IRI.
//!
//! Bridge to SparrowOntology: the minted IRI is persisted on the entity in the
//! reserved `__so_iri` property (`sparrowdb_ontology_core::namespace::SOURCE_IRI_KEY`),
//! which WS1's `export_data_turtle` already prefers over its own
//! `{base}/{Class}/{node_id}` minting. That is what makes vault → DB → Turtle
//! reproduce the vault's subject IRIs.

use crate::error::VaultError;

/// Mint a note's IRI from its governing base and file name.
///
/// Concatenates `governing_base` with the RFC 3987 percent-encoded file name
/// (spaces become `%20`; non-ASCII letters are left unencoded as IRI ucschar).
pub fn mint_iri(governing_base: &str, note_name: &str) -> Result<String, VaultError> {
    Ok(format!(
        "{governing_base}{}",
        percent_encode_iri_segment(note_name)
    ))
}

/// Percent-encode a file-name segment per RFC 3987.
///
/// Unreserved ASCII (`ALPHA / DIGIT / "-" / "." / "_" / "~"`) and non-ASCII
/// characters (IRI `ucschar`) pass through. Everything else is UTF-8
/// percent-encoded with uppercase hex.
pub fn percent_encode_iri_segment(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_ascii() {
            match c {
                'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '.' | '_' | '~' => out.push(c),
                _ => {
                    let mut buf = [0; 4];
                    for b in c.encode_utf8(&mut buf).as_bytes() {
                        out.push_str(&format!("%{b:02X}"));
                    }
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// An explicit `id` MUST be a full absolute `http(s)://` IRI; relative values
/// are non-conforming and reported by `check`.
pub fn is_conforming_explicit_id(id: &str) -> bool {
    id.starts_with("http://") || id.starts_with("https://")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_id_must_be_absolute() {
        assert!(is_conforming_explicit_id(
            "https://example.org/recipes/red-lentil-soup"
        ));
        assert!(!is_conforming_explicit_id("recipes/red-lentil-soup"));
    }

    #[test]
    fn mints_from_base_plus_encoded_file_name() {
        assert_eq!(
            mint_iri("https://example.org/", "hummus").unwrap(),
            "https://example.org/hummus"
        );
        assert_eq!(
            mint_iri("https://example.org/culinary#", "Recipe").unwrap(),
            "https://example.org/culinary#Recipe"
        );
        assert_eq!(
            mint_iri("https://example.org/", "Red Lentil Soup").unwrap(),
            "https://example.org/Red%20Lentil%20Soup"
        );
        assert_eq!(
            mint_iri("https://example.org/", "Café").unwrap(),
            "https://example.org/Café"
        );
    }
}
