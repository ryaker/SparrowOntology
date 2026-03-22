/// Reserved namespace prefix for all ontology metadata.
pub const ONTOLOGY_PREFIX: &str = "__SO_";

/// Label for ontology class metadata nodes.
pub const CLASS_LABEL: &str = "__SO_Class";

/// Label for ontology relation metadata nodes.
pub const RELATION_LABEL: &str = "__SO_Relation";

/// Label for ontology alias metadata nodes.
pub const ALIAS_LABEL: &str = "__SO_Alias";

/// Label for ontology property metadata nodes.
pub const PROPERTY_LABEL: &str = "__SO_Property";

/// Label for ontology constraint metadata nodes.
pub const CONSTRAINT_LABEL: &str = "__SO_Constraint";

// ============================================================================
// Edge types (relationships between ontology metadata nodes)
// ============================================================================

/// Edge from an alias to the canonical symbol it resolves to.
pub const ALIASES_TO: &str = "__SO_ALIASES_TO";

/// Edge from a relation to its domain class.
pub const DOMAIN: &str = "__SO_DOMAIN";

/// Edge from a relation to its range class.
pub const RANGE: &str = "__SO_RANGE";

/// Edge from a subclass to its parent class.
pub const SUBCLASS_OF: &str = "__SO_SUBCLASS_OF";

/// Edge from a subproperty to its parent property.
pub const SUBPROPERTY_OF: &str = "__SO_SUBPROPERTY_OF";

/// Edge from a property to its owning class.
pub const PROPERTY_OF: &str = "__SO_PROPERTY_OF";

/// Edge indicating a property is required on an entity.
pub const REQUIRED: &str = "__SO_REQUIRED";

// ============================================================================
// Properties on metadata nodes
// ============================================================================

/// The symbol name (canonical form, no aliases).
pub const PROP_NAME: &str = "name";

/// The symbol identifier (UUID).
pub const PROP_SYMBOL_ID: &str = "symbol_id";

/// The kind of symbol (Class, Relation, Property, etc.).
pub const PROP_KIND: &str = "kind";

/// Human-readable description.
pub const PROP_DESCRIPTION: &str = "description";

/// The canonical name this alias resolves to.
pub const PROP_CANONICAL_NAME: &str = "canonical_name";

/// The kind of alias (whether it aliases a Class or Relation).
pub const PROP_ALIAS_KIND: &str = "alias_kind";

/// The datatype of a property.
pub const PROP_DATATYPE: &str = "datatype";

/// Whether a property is optional or required.
pub const PROP_REQUIRED: &str = "is_required";

/// When this metadata was created.
pub const PROP_CREATED_AT: &str = "created_at";

/// When this metadata was last updated.
pub const PROP_UPDATED_AT: &str = "updated_at";

// ============================================================================
// Validation support
// ============================================================================

/// All reserved labels (used for checking conflicts).
pub const RESERVED_LABELS: &[&str] = &[
    CLASS_LABEL,
    RELATION_LABEL,
    ALIAS_LABEL,
    PROPERTY_LABEL,
    CONSTRAINT_LABEL,
];

/// All reserved edge types.
pub const RESERVED_RELS: &[&str] = &[
    ALIASES_TO,
    DOMAIN,
    RANGE,
    SUBCLASS_OF,
    SUBPROPERTY_OF,
    PROPERTY_OF,
    REQUIRED,
];

/// Properties allowed to start with `__so_` (the reserved property prefix).
/// All other properties starting with `__so_` are forbidden.
pub const ALLOWED_SO_PROPERTIES: &[&str] = &[
    "__so_source_label",
    "__so_source_rel",
];
