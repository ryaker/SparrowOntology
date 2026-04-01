//! Sparrow Ontology Core — Ontology semantic layer for SparrowDB
//!
//! This library provides:
//! - An ontology metadata model stored inside SparrowDB
//! - Semantic alias normalization (e.g., EMPLOYED_BY → WORKS_FOR)
//! - Write-time validation (domain/range, required properties, type checking)
//! - Hierarchy expansion (subclass/subproperty traversal)
//! - A guided world-model bootstrap (10 canonical classes, 19 relations, 22 properties)

pub mod error;
pub mod hierarchy;
pub mod init;
pub mod model;
pub mod namespace;
pub mod resolution;
pub mod validation;

pub use error::SoError;
pub use hierarchy::{check_no_cycle, expand_subclasses, expand_subproperties};
pub use init::{add_property, init, InitResult};
pub use model::*;
pub use resolution::{escape_cypher_string, list_canonical_names, resolve};
pub use validation::{is_subclass_of, ValidationContext};
