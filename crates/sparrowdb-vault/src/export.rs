//! Database → vault projection (§5.5 semantics, matching `rdf_to_vault.py`).
//!
//! One note per subject; hierarchy only in frontmatter; IRI objects that are
//! vault notes become `[[wiki links]]`, others CURIEs. Notes are updated **in
//! place**: bodies, `tags:`, and folder placement are preserved, never
//! regenerated from the graph.

use std::path::PathBuf;

use sparrowdb::GraphDb;

use crate::error::VaultError;
use crate::layout::Vault;

#[derive(Debug, Clone, Default)]
pub struct ExportOptions {
    /// Report the files that would change without writing them.
    pub dry_run: bool,
}

#[derive(Debug, Clone, Default)]
pub struct ExportReport {
    pub notes_created: Vec<PathBuf>,
    pub notes_updated: Vec<PathBuf>,
    pub notes_unchanged: usize,
    /// Constructs with no short-name mapping — flagged, never dropped (§5.6).
    pub unmapped: Vec<String>,
}

pub fn export(
    db: &GraphDb,
    vault: &Vault,
    opts: &ExportOptions,
) -> Result<ExportReport, VaultError> {
    let _ = (db, vault, opts);
    Err(VaultError::not_implemented("export", "export"))
}
