//! `check`: dry-run lint of a whole vault against the ontology.
//!
//! Errors match the actionable style of `create_entity`
//! ("Unknown property 'typo_field'. Valid: [...]"), carry file + line-adjacent
//! context, and a fix suggestion. Any error ⇒ nonzero exit from the CLI.

use std::path::PathBuf;

use sparrowdb::GraphDb;

use crate::error::VaultError;
use crate::layout::Vault;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiagnosticKind {
    /// Frontmatter is not valid YAML; the file is skipped, the run continues.
    MalformedFrontmatter,
    /// Key not resolvable through the composed context (host keys excepted).
    UnknownProperty,
    /// Relation target's class violates the relation's range.
    RelationRangeViolation,
    /// `[[Link]]` names no participating note (§4.4.1 MUST flag).
    DanglingWikiLink,
    /// Two participating notes share a file name without an explicit `id`.
    AmbiguousIdentity,
    /// `id` present but not an absolute IRI (§4.5).
    RelativeExplicitId,
    /// A note carries its own `@context` (§4.2 MUST NOT).
    InlineContext,
    /// Prefixed key such as `rdfs:comment:` (§4.3).
    PrefixedFieldName,
}

#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub file: PathBuf,
    /// 1-based line in the file, when the parser can attribute it.
    pub line: Option<usize>,
    pub field: Option<String>,
    pub kind: DiagnosticKind,
    pub message: String,
    pub suggestion: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct CheckReport {
    pub files_checked: usize,
    pub errors: Vec<Diagnostic>,
    pub warnings: Vec<Diagnostic>,
}

impl CheckReport {
    pub fn is_clean(&self) -> bool {
        self.errors.is_empty()
    }
}

pub fn check(db: &GraphDb, vault: &Vault) -> Result<CheckReport, VaultError> {
    let _ = (db, vault);
    Err(VaultError::not_implemented("check", "check"))
}
