//! `sparrow-ontology vault …` — thin CLI over the `sparrowdb-vault` crate.
//!
//! Scaffold: every subcommand parses its arguments, opens the vault and the
//! database, and surfaces the library's `NotImplemented` error (exit 1).

use std::path::{Path, PathBuf};

use clap::Subcommand;
use sparrowdb_vault::{ExportOptions, SchemaMergePolicy, SyncOptions, Vault, VaultError};

use super::open_db;

#[derive(Subcommand)]
pub enum VaultCommand {
    /// One-shot sync: vault notes → validated ontology + entities
    Sync {
        /// Vault root directory (contains context.jsonld)
        vault: PathBuf,
        #[arg(long)]
        db: PathBuf,
        /// Let vault schema notes replace conflicting DB definitions
        /// (default: merge and warn on conflicts)
        #[arg(long)]
        vault_authoritative: bool,
        /// Physically remove entities whose notes were deleted (default: soft delete)
        #[arg(long)]
        hard_delete: bool,
    },
    /// Watch the vault and re-sync incrementally on file changes
    Watch {
        vault: PathBuf,
        #[arg(long)]
        db: PathBuf,
        #[arg(long)]
        hard_delete: bool,
    },
    /// Project the database back into the vault, updating notes in place
    Export {
        vault: PathBuf,
        #[arg(long)]
        db: PathBuf,
        /// Report what would change without writing
        #[arg(long)]
        dry_run: bool,
    },
    /// Dry-run validation of the whole vault; nonzero exit on any error
    Check {
        vault: PathBuf,
        #[arg(long)]
        db: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Diff vault state against database state
    Drift {
        vault: PathBuf,
        #[arg(long)]
        db: PathBuf,
        #[arg(long)]
        json: bool,
    },
}

pub fn run(cmd: VaultCommand) -> Result<(), String> {
    match cmd {
        VaultCommand::Sync {
            vault,
            db,
            vault_authoritative,
            hard_delete,
        } => {
            let opts = SyncOptions {
                schema_policy: if vault_authoritative {
                    SchemaMergePolicy::VaultAuthoritative
                } else {
                    SchemaMergePolicy::MergeWithWarning
                },
                dry_run: false,
                hard_delete,
            };
            let (db, vault) = open(&db, &vault)?;
            sparrowdb_vault::sync(&db, &vault, &opts).map_err(render)?;
            Ok(())
        }
        VaultCommand::Watch {
            vault,
            db,
            hard_delete,
        } => {
            let opts = SyncOptions {
                hard_delete,
                ..SyncOptions::default()
            };
            let (db, vault) = open(&db, &vault)?;
            sparrowdb_vault::watch::watch(&db, &vault, &opts).map_err(render)
        }
        VaultCommand::Export { vault, db, dry_run } => {
            let (db, vault) = open(&db, &vault)?;
            sparrowdb_vault::export(&db, &vault, &ExportOptions { dry_run }).map_err(render)?;
            Ok(())
        }
        VaultCommand::Check { vault, db, json: _ } => {
            let (db, vault) = open(&db, &vault)?;
            let report = sparrowdb_vault::check(&db, &vault).map_err(render)?;
            if report.is_clean() {
                Ok(())
            } else {
                Err(format!("{} error(s)", report.errors.len()))
            }
        }
        VaultCommand::Drift { vault, db, json: _ } => {
            let (db, vault) = open(&db, &vault)?;
            sparrowdb_vault::drift(&db, &vault).map_err(render)?;
            Ok(())
        }
    }
}

fn open(db: &Path, vault: &Path) -> Result<(sparrowdb::GraphDb, Vault), String> {
    let vault = Vault::open(vault).map_err(render)?;
    Ok((open_db(db)?, vault))
}

fn render(e: VaultError) -> String {
    format!("Error: {e}")
}
