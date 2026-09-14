//! Vault tree walking and layer classification (§3, §5.1, §5.4 step 5).
//!
//! A note's *layer* comes from its folder: anything under `Ontologies/` or
//! `Vocabularies/` is schema, everything else is instance. Folders never
//! shape identity or hierarchy (§4.5, §5.2) — only the layer.

use std::fs;
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
    /// Hidden directories (name starts with `.`) are not entered.
    pub fn notes(&self) -> Result<Vec<NotePath>, VaultError> {
        let mut notes = Vec::new();
        collect_md(&self.root, &self.root, &mut notes)?;
        notes.sort_by(|a, b| a.relative.cmp(&b.relative));
        Ok(notes)
    }
}

fn collect_md(root: &Path, dir: &Path, out: &mut Vec<NotePath>) -> Result<(), VaultError> {
    let entries = fs::read_dir(dir).map_err(|source| VaultError::Io {
        path: dir.to_path_buf(),
        source,
    })?;
    let mut entries: Vec<_> =
        entries
            .collect::<Result<Vec<_>, _>>()
            .map_err(|source| VaultError::Io {
                path: dir.to_path_buf(),
                source,
            })?;
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.starts_with('.') {
            continue;
        }
        let path = entry.path();
        let ft = entry.file_type().map_err(|source| VaultError::Io {
            path: path.clone(),
            source,
        })?;
        if ft.is_symlink() {
            continue;
        }
        if ft.is_dir() {
            collect_md(root, &path, out)?;
            continue;
        }
        if ft.is_file() {
            if path.extension().and_then(|e| e.to_str()) == Some("md") {
                let relative = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
                out.push(NotePath { relative });
            } else {
                log::debug!("skipping non-Markdown vault file {}", path.display());
            }
        }
    }
    Ok(())
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

    #[test]
    fn notes_walks_md_skips_hidden_and_non_md() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("Items")).unwrap();
        fs::create_dir_all(dir.path().join(".obsidian")).unwrap();
        fs::write(dir.path().join("Items/sprocket.md"), "---\ntype: x\n---\n").unwrap();
        fs::write(dir.path().join("readme.txt"), "nope").unwrap();
        fs::write(dir.path().join("pic.png"), [0u8; 4]).unwrap();
        fs::write(dir.path().join(".obsidian/app.md"), "hidden").unwrap();
        fs::write(dir.path().join(".hidden.md"), "dotfile").unwrap();

        let vault = Vault::open(dir.path()).unwrap();
        let notes = vault.notes().unwrap();
        let rels: Vec<_> = notes
            .iter()
            .map(|n| n.relative.to_string_lossy().replace('\\', "/"))
            .collect();
        assert_eq!(rels, vec!["Items/sprocket.md"]);
    }
}
