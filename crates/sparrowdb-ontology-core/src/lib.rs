pub mod error;
pub mod hierarchy;
pub mod init;
pub mod model;
pub mod namespace;
pub mod resolution;
pub mod validation;

// ── Convenient re-exports ────────────────────────────────────────────────────

pub use error::SoError;
pub use init::{add_alias, add_property, define_subclass, init, InitResult, StarterKind};
pub use model::{
    AliasKind, OntologyAlias, OntologyClass, OntologyConstraint, OntologyProperty,
    OntologyRelation, OwnerKind, PropertyType, PropertyValue, SymbolStatus,
    canonical_world_model, canonical_world_model_properties, canonical_world_model_relations,
};
pub use resolution::{resolve, ResolvedSymbol};
pub use validation::ValidationContext;
