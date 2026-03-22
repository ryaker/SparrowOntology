/// Namespace prefix for all Sparrow Ontology nodes/edges.
/// TODO SPA-208: Replace convention-based protection with
/// `OpenOptions::reserve_label_prefix("__SO_")` when SPA-208 ships.
pub const SO_NAMESPACE: &str = "__SO_";

// ── Node labels ──────────────────────────────────────────────────────────────

pub const CLASS_LABEL: &str = "__SO_Class";
pub const RELATION_LABEL: &str = "__SO_Relation";
pub const PROPERTY_LABEL: &str = "__SO_Property";
pub const CONSTRAINT_LABEL: &str = "__SO_Constraint";
pub const ALIAS_LABEL: &str = "__SO_Alias";

// ── Edge types ───────────────────────────────────────────────────────────────

pub const ALIAS_OF_REL: &str = "__SO_ALIAS_OF";
pub const SUBCLASS_OF_REL: &str = "__SO_SUBCLASS_OF";
pub const SUBPROPERTY_OF_REL: &str = "__SO_SUBPROPERTY_OF";
pub const HAS_PROPERTY_REL: &str = "__SO_HAS_PROPERTY";
pub const HAS_CONSTRAINT_REL: &str = "__SO_HAS_CONSTRAINT";
pub const DOMAIN_REL: &str = "__SO_DOMAIN";
pub const RANGE_REL: &str = "__SO_RANGE";

// ── Reserved property keys on user nodes/edges (provenance) ─────────────────

/// Allowed on user nodes when preserve_source_terms=true and was_alias=true.
pub const SOURCE_LABEL_KEY: &str = "__so_source_label";
/// Allowed on user edges when preserve_source_terms=true and was_alias=true.
pub const SOURCE_REL_KEY: &str = "__so_source_rel";
