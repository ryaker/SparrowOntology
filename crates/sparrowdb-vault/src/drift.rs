//! `drift`: vault state vs database state.
//!
//! Needs a sync manifest (per-note content hash + last-synced DB state) so it
//! can tell "changed on disk since last sync" from "changed in DB since last
//! export". Where that manifest lives (sidecar file in the vault vs reserved
//! nodes in the DB) is an open question — see README.

use std::path::PathBuf;

use sparrowdb::GraphDb;

use crate::error::VaultError;
use crate::layout::Vault;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriftSide {
    /// Note changed on disk since last sync.
    Vault,
    /// Entity changed in the DB since last export.
    Database,
    /// Both sides changed — needs a human.
    Conflict,
}

#[derive(Debug, Clone)]
pub struct DriftEntry {
    pub note: PathBuf,
    pub iri: Option<String>,
    pub side: DriftSide,
    pub detail: String,
}

#[derive(Debug, Clone, Default)]
pub struct DriftReport {
    pub entries: Vec<DriftEntry>,
}

pub fn drift(db: &GraphDb, vault: &Vault) -> Result<DriftReport, VaultError> {
    let _ = (db, vault);
    Err(VaultError::not_implemented("drift", "drift"))
}
