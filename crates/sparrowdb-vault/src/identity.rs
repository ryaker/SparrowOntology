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
pub fn mint_iri(governing_base: &str, note_name: &str) -> Result<String, VaultError> {
    let _ = (governing_base, note_name);
    Err(VaultError::not_implemented("identity::mint_iri", "parse"))
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
}
