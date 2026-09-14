//! Vault-LD engine for Sparrow Ontology (WS2).
//!
//! [Vault-LD](https://vault-ld.org/) (spec v0.5.0) defines how a directory of
//! Markdown notes reads as an RDF graph: YAML frontmatter is YAML-LD resolved
//! through a composed `context.jsonld`, `[[Wiki Links]]` are edges, and the
//! note body stays prose. It ships a format and two batch converters, but no
//! database, write-time validation, incremental sync, or drift detection.
//! This crate is that engine, on top of SparrowOntology + SparrowDB.
//!
//! # Status: parse + DB-free `check`
//!
//! Frontmatter YAML, vault walking, context composition, IRI minting, and
//! DB-free `check` lints are implemented. Sync/export/drift/watch still
//! return [`VaultError::NotImplemented`].
//!
//! # Module map (Vault-LD section → module)
//!
//! | Vault-LD concept                         | Module          |
//! |------------------------------------------|-----------------|
//! | vault tree, schema vs instance layer §3 §5.4.5 | [`layout`] |
//! | composed context, scoped `@base` §4.2    | [`context`]     |
//! | frontmatter + wiki links §4.1 §4.3 §4.4  | [`parse`]       |
//! | identity / IRI minting §4.5              | [`identity`]    |
//! | vault → DB (`sync`) §5.4 semantics       | [`sync`]        |
//! | DB → vault (`export`) §5.5               | [`export`]      |
//! | dry-run validation (`check`) §6          | [`check`]       |
//! | vault vs DB diff (`drift`)               | [`drift`]       |
//! | incremental re-sync (`watch`)            | [`watch`]       |
//!
//! See `crates/sparrowdb-vault/README.md` for the plan and open questions.

pub mod check;
pub mod context;
pub mod drift;
pub mod error;
pub mod export;
pub mod identity;
pub mod layout;
pub mod parse;
pub mod sync;
pub mod watch;

pub use check::{check, CheckReport, Diagnostic, DiagnosticKind};
pub use context::ComposedContext;
pub use drift::{drift, DriftEntry, DriftReport, DriftSide};
pub use error::VaultError;
pub use export::{export, ExportOptions, ExportReport};
pub use identity::mint_iri;
pub use layout::{Layer, NoteKind, NotePath, Vault};
pub use parse::{split_frontmatter, Frontmatter, FrontmatterValue, WikiLink};
pub use sync::{sync, SchemaMergePolicy, SyncOptions, SyncReport};

/// Vault-LD spec version this crate targets.
pub const VAULT_LD_SPEC_VERSION: &str = "0.5.0";

/// Vault-LD's own namespace; `vld:path` is its only term (§5.4 step 7).
pub const VLD_NS: &str = "https://github.com/The-Knowledge-Graph-Guys/vault-ld#";

/// Entity property that holds a note's Markdown body so SparrowDB's inverted
/// text index can search it. Bodies flow vault → DB only (§5.3; WS2 non-goal).
pub const BODY_PROPERTY: &str = "_body";
