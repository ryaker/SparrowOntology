/// All errors that sparrowdb-ontology-core can return.
#[derive(Debug, thiserror::Error)]
pub enum SoError {
    #[error(
        "'{0}' starts with '__SO_' which is reserved for internal use. Choose a different name."
    )]
    ReservedNamespace(String),

    #[error("'{0}' starts with '__so_' which is reserved for system use. Use a different property name. Call explain_symbol on the class to see already-declared properties.")]
    ReservedProperty(String),

    #[error("Unknown {kind} '{name}'. Valid: {valid:?}")]
    UnknownSymbol {
        name: String,
        kind: String,
        valid: Vec<String>,
        closest_match: Option<String>,
        suggestion: Option<String>,
    },

    #[error("Alias '{alias}' is already registered for '{existing}'. Use '{existing}' directly, or call get_ontology to see all registered aliases.")]
    AliasConflict {
        alias: String,
        existing: String,
        kind: String,
    },

    #[error("Adding '{child}' → '{parent}' would create a cycle in the subclass hierarchy. Call explain_symbol('{child}') to see its existing hierarchy before adding this relation.")]
    CycleDetected { child: String, parent: String },

    #[error("Relation '{relation}' requires the source entity to be of class '{expected}', but got '{actual}'. Call explain_symbol('{relation}') to see full domain/range constraints.")]
    DomainViolation {
        relation: String,
        expected: String,
        actual: String,
    },

    #[error("Relation '{relation}' requires the target entity to be of class '{expected}', but got '{actual}'. Call explain_symbol('{relation}') to see full domain/range constraints.")]
    RangeViolation {
        relation: String,
        expected: String,
        actual: String,
    },

    #[error("Property '{property}' is required on class '{class}' but was not provided. Add it to your create_entity call, or call add_property(owner='{class}', name='{property}', required=false) to make it optional.")]
    RequiredPropertyMissing { class: String, property: String },

    #[error("Type mismatch: '{property}' on '{class}' expects {expected}, got {actual}. Call explain_symbol('{class}') to see all property types.")]
    TypeMismatch {
        class: String,
        property: String,
        expected: String,
        actual: String,
    },

    #[error("Property '{property}' is already declared on '{class}'. Call explain_symbol('{class}') to see all existing properties.")]
    DuplicateProperty { class: String, property: String },

    #[error("Class '{class_name}' has no declared properties. Call add_property(owner='{class_name}', name='...') for each property before writing entities. Call start_here to see all unseeded_classes.")]
    UnseedeedClass { class_name: String },

    #[error("Property '{property}' on '{class}' only allows values {allowed:?}, but got '{value}'. Call explain_symbol('{class}') to see declared properties.")]
    EnumViolation {
        class: String,
        property: String,
        value: String,
        allowed: Vec<String>,
    },

    #[error("Ontology already initialized. Use force=true to reset (destructive).")]
    AlreadyInitialized,

    #[error("Storage: {0}")]
    Storage(#[from] sparrowdb_common::Error),
}
