//! Vault tree walking and layer classification (§3, §5.1, §5.4 step 5).
//!
//! A note's *layer* comes from its folder: anything under `Ontologies/` or
//! `Vocabularies/` is schema, everything else is instance. Folders never
//! shape identity or hierarchy (§4.5, §5.2) — only the layer.

use std::path::{Path, PathBuf};

use crate::error::VaultError;

pub const ONTOLOGIES_DIR: &str = "Ontologies";
pub const VOCABULARIES_DIR: &str = "Vocabularies";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layer {
    Schema,
    Instance,
}

/// What a note declares itself to be. Decided from `@type` after parsing,
/// not from the folder — `Classes/` and `Properties/` are shelving.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NoteKind {
    /// `owl:Ontology` — no SparrowOntology counterpart yet (see README).
    Ontology,
    /// `owl:Class` → `define_class`.
    Class,
    /// `owl:ObjectProperty` → `define_relation`.
    ObjectProperty,
    /// `owl:DatatypeProperty` → `add_property`.
    DatatypeProperty,
    /// `skos:ConceptScheme` — no SparrowOntology counterpart yet.
    ConceptScheme,
    /// `skos:Concept` — no SparrowOntology counterpart yet.
    Concept,
    /// Typed instance note (`type: "[[Recipe]]"`) → validated entity.
    Instance { class_link: String },
}

/// A Markdown file inside a vault, addressed by its vault-relative path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotePath {
    pub relative: PathBuf,
}

impl NotePath {
    /// Layer by first path component (§5.4 step 5).
    pub fn layer(&self) -> Layer {
        match self.relative.components().next() {
            Some(c) if c.as_os_str() == ONTOLOGIES_DIR || c.as_os_str() == VOCABULARIES_DIR => {
                Layer::Schema
            }
            _ => Layer::Instance,
        }
    }

    /// File name without `.md` — the only input to minted identity (§4.5).
    pub fn note_name(&self) -> Option<&str> {
        self.relative.file_stem().and_then(|s| s.to_str())
    }
}

/// An opened vault root.
#[derive(Debug, Clone)]
pub struct Vault {
    pub root: PathBuf,
}

impl Vault {
    pub fn open(root: impl AsRef<Path>) -> Result<Self, VaultError> {
        let root = root.as_ref().to_path_buf();
        if !root.is_dir() {
            return Err(VaultError::VaultNotFound(root));
        }
        Ok(Vault { root })
    }

    /// Every `.md` note in the vault. Non-Markdown files (images, PDFs, …)
    /// are skipped and logged at debug level — spec open question default.
    pub fn notes(&self) -> Result<Vec<NotePath>, VaultError> {
        Err(VaultError::not_implemented("Vault::notes", "parse"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(p: &str) -> NotePath {
        NotePath {
            relative: PathBuf::from(p),
        }
    }

    #[test]
    fn layer_is_decided_by_top_folder() {
        assert_eq!(
            note("Ontologies/Culinary/Classes/Recipe.md").layer(),
            Layer::Schema
        );
        assert_eq!(
            note("Vocabularies/DifficultyLevels/Beginner.md").layer(),
            Layer::Schema
        );
        assert_eq!(note("Recipes/hummus.md").layer(), Layer::Instance);
        // A folder merely *named* like a schema folder deeper down is shelving.
        assert_eq!(
            note("Recipes/Ontologies/hummus.md").layer(),
            Layer::Instance
        );
    }

    #[test]
    fn note_name_drops_folder_and_extension() {
        assert_eq!(
            note("Recipes/Soups/Red Lentil Soup.md").note_name(),
            Some("Red Lentil Soup")
        );
    }
}
