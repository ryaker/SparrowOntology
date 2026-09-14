//! `watch`: incremental re-sync on file change.
//!
//! Planned: `notify` behind a cargo feature so embedded users who only call
//! `sync` don't pull a file-watcher. Handles edit / create / delete / rename
//! within one sync cycle; unchanged files skipped by content hash; deletes are
//! soft unless `SyncOptions::hard_delete`.

use sparrowdb::GraphDb;

use crate::error::VaultError;
use crate::layout::Vault;
use crate::sync::SyncOptions;

pub fn watch(db: &GraphDb, vault: &Vault, opts: &SyncOptions) -> Result<(), VaultError> {
    let _ = (db, vault, opts);
    Err(VaultError::not_implemented("watch", "watch"))
}
