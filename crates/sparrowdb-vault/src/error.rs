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

    #[error("malformed YAML frontmatter in {path}{}: {message}", line_suffix(*.line))]
    MalformedYaml {
        path: PathBuf,
        /// 1-based line in the YAML block (not the file) when the parser can attribute it.
        line: Option<usize>,
        message: String,
    },

    #[error("malformed JSON in {path}: {message}")]
    MalformedJson { path: PathBuf, message: String },

    /// Spec §2.6: no runtime network access. A `http(s)://` context ref is refused.
    #[error("remote context ref {href} in {from} (network access is forbidden)")]
    RemoteContext { from: PathBuf, href: String },

    #[error("context composition cycle involving {0}")]
    ContextCycle(PathBuf),

    #[error("io error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error(transparent)]
    Ontology(#[from] SoError),
}

fn line_suffix(line: Option<usize>) -> String {
    match line {
        Some(n) => format!(":{n}"),
        None => String::new(),
    }
}

impl VaultError {
    pub(crate) fn not_implemented(operation: &'static str, slice: &'static str) -> Self {
        VaultError::NotImplemented { operation, slice }
    }
}
