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
use std::path::{Path, PathBuf};

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

impl ComposedContext {
    /// Load and compose starting from `<vault>/context.jsonld`.
    pub fn load(vault_root: &Path) -> Result<Self, VaultError> {
        let _ = vault_root;
        Err(VaultError::not_implemented(
            "ComposedContext::load",
            "parse",
        ))
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
