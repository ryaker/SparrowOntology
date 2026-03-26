pub mod error;
pub mod hierarchy;
pub mod import;
pub mod init;
pub mod jsonld;
pub mod model;
pub mod namespace;
pub mod resolution;
pub mod snapshot;
pub mod turtle_import;
pub mod validation;

// ── Convenient re-exports ────────────────────────────────────────────────────

pub use error::SoError;
pub use import::{import_records, ImportError, ImportResult, ImportTemplate};
pub use init::{add_alias, add_property, define_subclass, init, InitResult, StarterKind};
pub use jsonld::export_json_ld;
pub use model::{
    canonical_world_model, canonical_world_model_properties, canonical_world_model_relations,
    personal_knowledge_classes, personal_knowledge_properties, personal_knowledge_relations,
    professional_network_classes, professional_network_properties, professional_network_relations,
    research_notes_classes, research_notes_properties, research_notes_relations, AliasKind,
    OntologyAlias, OntologyClass, OntologyProperty, OntologyRelation, OwnerKind, PropertyType,
    PropertyValue, SymbolStatus,
};
pub use resolution::{resolve, ResolvedSymbol};
pub use snapshot::{
    export_schema, import_schema, ImportSchemaResult, SchemaSnapshot, SNAPSHOT_VERSION,
};
pub use turtle_import::{import_turtle, DomainRangeStrategy, ImportOptions, ImportSummary};
pub use validation::{
    validate, ValidationContext, ValidationReport, ValidationViolation, ValidationWarning,
    ViolationKind,
};
