//! One-shot vault → database sync.
//!
//! Planned projection:
//! - class notes → `define_class` (+ `define_subclass` from `subClassOf`)
//! - object-property notes → `define_relation` (domain/range from frontmatter)
//! - datatype-property notes → `add_property`
//! - `label` / host `aliases` → `add_alias`
//! - instance notes → validated entities via the existing write path, subject
//!   IRI persisted in `__so_iri`, body stored in [`crate::BODY_PROPERTY`]
//! - wiki-link values → validated relationships
//!
//! Reuse `sparrowdb_ontology_core::turtle_import`'s owl/rdfs → ontology mapping
//! rather than re-deriving it here.

use sparrowdb::GraphDb;

use crate::error::VaultError;
use crate::layout::Vault;

/// What to do when a schema note disagrees with an ontology already in the DB.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SchemaMergePolicy {
    /// Spec §7 default: merge, warn on every conflict, never overwrite silently.
    #[default]
    MergeWithWarning,
    /// Vault schema notes win; conflicting DB definitions are replaced.
    VaultAuthoritative,
}

#[derive(Debug, Clone, Default)]
pub struct SyncOptions {
    pub schema_policy: SchemaMergePolicy,
    /// Validate and report, write nothing (what `check` uses).
    pub dry_run: bool,
    /// Deletes are soft (entity flagged) unless this is set.
    pub hard_delete: bool,
}

#[derive(Debug, Clone, Default)]
pub struct SyncReport {
    pub notes_seen: usize,
    pub notes_unchanged: usize,
    pub classes_defined: usize,
    pub relations_defined: usize,
    pub properties_added: usize,
    pub entities_written: usize,
    pub relationships_written: usize,
    /// Files skipped with a reason (bad YAML, validation failure, …).
    pub skipped: Vec<(String, String)>,
    pub warnings: Vec<String>,
}

pub fn sync(db: &GraphDb, vault: &Vault, opts: &SyncOptions) -> Result<SyncReport, VaultError> {
    let _ = (db, vault, opts);
    Err(VaultError::not_implemented(
        "sync",
        "sync-schema, then sync-instances",
    ))
}
