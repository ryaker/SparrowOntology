use thiserror::Error;

/// Errors that can occur in Sparrow Ontology operations.
///
/// Each variant includes enough context to help users understand what went wrong
/// and what they can do about it (Invisible Teacher pattern).
#[derive(Error, Debug, Clone)]
pub enum SoError {
    /// Attempted to use or create a symbol in the reserved `__SO_` namespace.
    #[error("ReservedNamespace: '{name}' uses reserved prefix '__SO_'. This namespace is reserved for ontology metadata.")]
    ReservedNamespace { name: String },

    /// Attempted to create or modify a property starting with `__so_` outside allowed whitelist.
    #[error("ReservedProperty: '{property}' uses reserved property prefix '__so_'. Allowed: __so_source_label, __so_source_rel.")]
    ReservedProperty { property: String },

    /// A symbol (class, relation, alias, or property) was not found.
    #[error("UnknownSymbol: '{symbol}' ({kind:?}) not found. Valid options: {valid_options}. You can register '{symbol}' as an alias or add it as a subclass.")]
    UnknownSymbol {
        symbol: String,
        kind: String,  // "Class", "Relation", "Property", "Alias"
        valid_options: String,  // comma-separated list
    },

    /// Two different symbols are aliased to the same canonical name.
    #[error("AliasConflict: '{name1}' and '{name2}' both alias to '{canonical}' ({kind:?}). Only one is allowed.")]
    AliasConflict {
        name1: String,
        name2: String,
        canonical: String,
        kind: String,
    },

    /// A cycle was detected in the subclass or subproperty hierarchy.
    #[error("CycleDetected: Subclass cycle detected: {child} → {parent} (via {edge_type}). Hierarchy must be acyclic.")]
    CycleDetected {
        child: String,
        parent: String,
        edge_type: String,
    },

    /// A relationship violates domain constraints.
    #[error("DomainViolation: Relationship '{relation}' has domain '{expected_domain}', but source is '{actual_source}'. Valid sources: {valid_sources}.")]
    DomainViolation {
        relation: String,
        expected_domain: String,
        actual_source: String,
        valid_sources: String,  // comma-separated
    },

    /// A relationship violates range constraints.
    #[error("RangeViolation: Relationship '{relation}' has range '{expected_range}', but target is '{actual_target}'. Valid targets: {valid_targets}.")]
    RangeViolation {
        relation: String,
        expected_range: String,
        actual_target: String,
        valid_targets: String,  // comma-separated
    },

    /// A required property is missing from an entity.
    #[error("RequiredPropertyMissing: Entity of type '{entity_type}' is missing required property '{property}'. Required properties: {required_properties}.")]
    RequiredPropertyMissing {
        entity_type: String,
        property: String,
        required_properties: String,  // comma-separated
    },

    /// A property value has the wrong type.
    #[error("TypeMismatch: Property '{property}' expects {expected_type}, but got {actual_type}. Example: {example_value}.")]
    TypeMismatch {
        property: String,
        expected_type: String,
        actual_type: String,
        example_value: String,
    },

    /// Attempted to initialize when the ontology is already initialized.
    #[error("AlreadyInitialized: Ontology is already initialized with {class_count} classes, {relation_count} relations, and {property_count} properties. Use init(..., force=true) to reinitialize.")]
    AlreadyInitialized {
        class_count: usize,
        relation_count: usize,
        property_count: usize,
    },

    /// A storage error occurred (database unavailable, disk full, etc.).
    #[error("Storage error: {message}")]
    Storage { message: String },
}

impl SoError {
    /// Create a UnknownSymbol error with a properly formatted valid_options string.
    pub fn unknown_symbol(symbol: impl Into<String>, kind: impl Into<String>, valid: Vec<impl Into<String>>) -> Self {
        let valid_str = valid
            .into_iter()
            .map(|s| s.into())
            .collect::<Vec<_>>()
            .join(", ");
        SoError::UnknownSymbol {
            symbol: symbol.into(),
            kind: kind.into(),
            valid_options: valid_str,
        }
    }

    /// Create a DomainViolation error with proper formatting.
    pub fn domain_violation(
        relation: impl Into<String>,
        expected_domain: impl Into<String>,
        actual_source: impl Into<String>,
        valid_sources: Vec<impl Into<String>>,
    ) -> Self {
        let valid_str = valid_sources
            .into_iter()
            .map(|s| s.into())
            .collect::<Vec<_>>()
            .join(", ");
        SoError::DomainViolation {
            relation: relation.into(),
            expected_domain: expected_domain.into(),
            actual_source: actual_source.into(),
            valid_sources: valid_str,
        }
    }

    /// Create a RangeViolation error with proper formatting.
    pub fn range_violation(
        relation: impl Into<String>,
        expected_range: impl Into<String>,
        actual_target: impl Into<String>,
        valid_targets: Vec<impl Into<String>>,
    ) -> Self {
        let valid_str = valid_targets
            .into_iter()
            .map(|s| s.into())
            .collect::<Vec<_>>()
            .join(", ");
        SoError::RangeViolation {
            relation: relation.into(),
            expected_range: expected_range.into(),
            actual_target: actual_target.into(),
            valid_targets: valid_str,
        }
    }

    /// Create a RequiredPropertyMissing error with proper formatting.
    pub fn required_property_missing(
        entity_type: impl Into<String>,
        property: impl Into<String>,
        required_properties: Vec<impl Into<String>>,
    ) -> Self {
        let required_str = required_properties
            .into_iter()
            .map(|s| s.into())
            .collect::<Vec<_>>()
            .join(", ");
        SoError::RequiredPropertyMissing {
            entity_type: entity_type.into(),
            property: property.into(),
            required_properties: required_str,
        }
    }
}
