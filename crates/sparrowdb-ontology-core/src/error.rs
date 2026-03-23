/// All errors that sparrowdb-ontology-core can return.
#[derive(Debug, thiserror::Error)]
pub enum SoError {
    #[error("Reserved namespace: '{0}' is reserved")]
    ReservedNamespace(String),

    #[error("Reserved property: '{0}' cannot be set by callers")]
    ReservedProperty(String),

    #[error("Unknown {kind} '{name}'. Valid: {valid:?}")]
    UnknownSymbol {
        name: String,
        kind: String,
        valid: Vec<String>,
    },

    #[error("Alias '{alias}' already registered for '{existing}' ({kind})")]
    AliasConflict {
        alias: String,
        existing: String,
        kind: String,
    },

    #[error("Cycle: adding '{child}' → '{parent}' would create a cycle")]
    CycleDetected { child: String, parent: String },

    #[error("Domain violation: '{relation}' requires source '{expected}', got '{actual}'")]
    DomainViolation {
        relation: String,
        expected: String,
        actual: String,
    },

    #[error("Range violation: '{relation}' requires target '{expected}', got '{actual}'")]
    RangeViolation {
        relation: String,
        expected: String,
        actual: String,
    },

    #[error("Required property '{property}' missing on '{class}'")]
    RequiredPropertyMissing { class: String, property: String },

    #[error("Type mismatch: '{property}' on '{class}' expects {expected}, got {actual}")]
    TypeMismatch {
        class: String,
        property: String,
        expected: String,
        actual: String,
    },

    #[error("Property '{property}' already declared on class '{class}'")]
    DuplicateProperty { class: String, property: String },

    #[error("Class '{class_name}' has no declared properties. Call add_property(owner='{class_name}', name='...') for each property before writing entities. Call start_here to see all unseeded_classes.")]
    UnseedeedClass { class_name: String },

    #[error("Ontology already initialized. Use force=true to reset (destructive).")]
    AlreadyInitialized,

    #[error("Storage: {0}")]
    Storage(#[from] sparrowdb_common::Error),
}
