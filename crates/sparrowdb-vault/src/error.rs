use std::path::PathBuf;

use sparrowdb_ontology_core::SoError;

#[derive(Debug, thiserror::Error)]
pub enum VaultError {
    /// Scaffold placeholder; names the WS2 slice that will implement it.
    #[error("sparrowdb-vault: `{operation}` is not implemented yet (planned slice: {slice})")]
    NotImplemented {
        operation: &'static str,
        slice: &'static str,
    },

    #[error("vault root not found or not a directory: {0}")]
    VaultNotFound(PathBuf),

    /// The vault-root `context.jsonld` is the one mandatory context (§4.5).
    #[error("missing root context.jsonld in vault {0}")]
    MissingRootContext(PathBuf),

    #[error("io error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error(transparent)]
    Ontology(#[from] SoError),
}

impl VaultError {
    pub(crate) fn not_implemented(operation: &'static str, slice: &'static str) -> Self {
        VaultError::NotImplemented { operation, slice }
    }
}
