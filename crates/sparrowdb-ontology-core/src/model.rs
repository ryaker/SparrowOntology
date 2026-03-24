use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

use crate::error::SoError;

// ============================================================================
// Enums
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SymbolStatus {
    Active,
    Deprecated,
    Reserved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AliasKind {
    Class,
    Relation,
    Property,
}

impl std::fmt::Display for AliasKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AliasKind::Class => write!(f, "Class"),
            AliasKind::Relation => write!(f, "Relation"),
            AliasKind::Property => write!(f, "Property"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PropertyType {
    String,
    Integer,
    Float,
    Boolean,
    DateTime,
    Json,
}

impl std::fmt::Display for PropertyType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PropertyType::String => write!(f, "String"),
            PropertyType::Integer => write!(f, "Integer"),
            PropertyType::Float => write!(f, "Float"),
            PropertyType::Boolean => write!(f, "Boolean"),
            PropertyType::DateTime => write!(f, "DateTime"),
            PropertyType::Json => write!(f, "Json"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PropertyValue {
    String(String),
    Integer(i64),
    Float(f64),
    Boolean(bool),
    DateTime(String),  // ISO 8601
    Json(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OwnerKind {
    Class,
    Relation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConstraintKind {
    Required,
    Unique,
    Pattern,
    Range,
}

// ============================================================================
// Type Definitions
// ============================================================================

/// An ontology class represents a category of entities.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OntologyClass {
    pub name: String,
    pub symbol_id: String,  // UUID string
    pub description: Option<String>,
    pub status: SymbolStatus,
}

impl OntologyClass {
    pub fn new(name: impl Into<String>, description: Option<impl Into<String>>) -> Self {
        OntologyClass {
            name: name.into(),
            symbol_id: Uuid::new_v4().to_string(),
            description: description.map(|d| d.into()),
            status: SymbolStatus::Active,
        }
    }
}

/// An ontology relation (edge type) connects two classes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OntologyRelation {
    pub name: String,
    pub symbol_id: String,  // UUID string
    pub domain: String,     // Class name
    pub range: String,      // Class name
    pub description: Option<String>,
    pub status: SymbolStatus,
}

impl OntologyRelation {
    pub fn new(
        name: impl Into<String>,
        domain: impl Into<String>,
        range: impl Into<String>,
        description: Option<impl Into<String>>,
    ) -> Self {
        OntologyRelation {
            name: name.into(),
            symbol_id: Uuid::new_v4().to_string(),
            domain: domain.into(),
            range: range.into(),
            description: description.map(|d| d.into()),
            status: SymbolStatus::Active,
        }
    }
}

/// An ontology alias allows alternative names for canonical symbols.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OntologyAlias {
    pub name: String,
    pub symbol_id: String,     // UUID string
    pub canonical_name: String, // The symbol this aliases to
    pub alias_kind: AliasKind,
    pub description: Option<String>,
}

impl OntologyAlias {
    pub fn new(
        name: impl Into<String>,
        canonical_name: impl Into<String>,
        alias_kind: AliasKind,
        description: Option<impl Into<String>>,
    ) -> Self {
        OntologyAlias {
            name: name.into(),
            symbol_id: Uuid::new_v4().to_string(),
            canonical_name: canonical_name.into(),
            alias_kind,
            description: description.map(|d| d.into()),
        }
    }
}

/// An ontology property describes attributes of entities or relationships.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OntologyProperty {
    pub name: String,
    pub symbol_id: String,      // UUID string
    pub owner: String,           // Class or Relation name
    pub owner_kind: OwnerKind,
    pub datatype: PropertyType,
    pub is_required: bool,
    pub description: Option<String>,
}

impl OntologyProperty {
    pub fn required(
        owner: impl Into<String>,
        owner_kind: OwnerKind,
        name: impl Into<String>,
        datatype: PropertyType,
        description: Option<impl Into<String>>,
    ) -> Self {
        OntologyProperty {
            name: name.into(),
            symbol_id: Uuid::new_v4().to_string(),
            owner: owner.into(),
            owner_kind,
            datatype,
            is_required: true,
            description: description.map(|d| d.into()),
        }
    }

    pub fn optional(
        owner: impl Into<String>,
        owner_kind: OwnerKind,
        name: impl Into<String>,
        datatype: PropertyType,
        description: Option<impl Into<String>>,
    ) -> Self {
        OntologyProperty {
            name: name.into(),
            symbol_id: Uuid::new_v4().to_string(),
            owner: owner.into(),
            owner_kind,
            datatype,
            is_required: false,
            description: description.map(|d| d.into()),
        }
    }
}

/// An ontology constraint enforces validation rules on entities or relationships.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OntologyConstraint {
    pub name: String,
    pub symbol_id: String,
    pub target_property: String,
    pub constraint_kind: ConstraintKind,
    pub rule: String,  // Constraint-specific rule (e.g., regex pattern, min/max range)
}

impl OntologyConstraint {
    pub fn new(
        name: impl Into<String>,
        target_property: impl Into<String>,
        constraint_kind: ConstraintKind,
        rule: impl Into<String>,
    ) -> Self {
        OntologyConstraint {
            name: name.into(),
            symbol_id: Uuid::new_v4().to_string(),
            target_property: target_property.into(),
            constraint_kind,
            rule: rule.into(),
        }
    }
}

/// Result of resolving a symbol (class, relation, property) name.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedSymbol {
    pub canonical_name: String,
    pub symbol_id: String,
    pub kind: AliasKind,
    pub was_alias: bool,
    pub description: Option<String>,
}

// ============================================================================
// Canonical World Model
// ============================================================================

/// Returns the 10 canonical world model classes.
///
/// These classes form the foundation of the ontology and scale from
/// individual to enterprise:
///
/// - Person: An individual agent (employee, contractor, founder, etc.)
/// - Organization: Any entity that groups people (company, team, etc.)
/// - Project: A bounded set of work with a goal
/// - Task: A unit of work (issue, ticket, to-do)
/// - Role: A function someone plays (CEO, engineer, researcher, etc.)
/// - Event: Something that happened (meeting, milestone, deadline)
/// - Decision: A choice made that affects the organization
/// - Policy: A rule or principle that guides behavior
/// - Concept: An abstract idea or piece of knowledge
/// - Dependency: A blocking or coupling relationship
pub fn canonical_world_model() -> Vec<OntologyClass> {
    vec![
        OntologyClass::new("Person", Some("An individual agent (employee, contractor, founder)")),
        OntologyClass::new(
            "Organization",
            Some("Any entity that groups people (company, team, department)"),
        ),
        OntologyClass::new(
            "Project",
            Some("A bounded set of work with a goal and timeline"),
        ),
        OntologyClass::new("Task", Some("A unit of work (issue, ticket, to-do)")),
        OntologyClass::new("Role", Some("A function or responsibility someone plays")),
        OntologyClass::new(
            "Event",
            Some("Something that happened (meeting, milestone, deadline)"),
        ),
        OntologyClass::new(
            "Decision",
            Some("A choice made that affects the organization"),
        ),
        OntologyClass::new(
            "Policy",
            Some("A rule or principle that guides behavior"),
        ),
        OntologyClass::new(
            "Concept",
            Some("An abstract idea or piece of knowledge"),
        ),
        OntologyClass::new(
            "Dependency",
            Some("A blocking or coupling relationship between work items"),
        ),
    ]
}

/// Returns the 19 canonical world model relations.
pub fn canonical_world_model_relations() -> Vec<OntologyRelation> {
    vec![
        OntologyRelation::new("KNOWS", "Person", "Person", Some("Person knows another person")),
        OntologyRelation::new(
            "WORKS_FOR",
            "Person",
            "Organization",
            Some("Person is employed by an organization"),
        ),
        OntologyRelation::new(
            "WORKS_WITH",
            "Person",
            "Person",
            Some("Person collaborates with another person"),
        ),
        OntologyRelation::new(
            "OWNS",
            "Person",
            "Project",
            Some("Person owns or leads a project"),
        ),
        OntologyRelation::new(
            "BELONGS_TO",
            "Project",
            "Organization",
            Some("Project belongs to an organization"),
        ),
        OntologyRelation::new(
            "DEPENDS_ON",
            "Task",
            "Task",
            Some("Task is blocked by another task"),
        ),
        OntologyRelation::new(
            "RELATED_TO",
            "Concept",
            "Concept",
            Some("Concept is related to another concept"),
        ),
        OntologyRelation::new(
            "DECIDED",
            "Decision",
            "Policy",
            Some("Decision created or modified a policy"),
        ),
        OntologyRelation::new(
            "FOLLOWS",
            "Person",
            "Policy",
            Some("Person adheres to a policy"),
        ),
        OntologyRelation::new(
            "PARTICIPATES_IN",
            "Person",
            "Event",
            Some("Person attended or participated in an event"),
        ),
        OntologyRelation::new(
            "HAS_ROLE",
            "Person",
            "Role",
            Some("Person has a role in an organization"),
        ),
        OntologyRelation::new(
            "ASSIGNED_TO",
            "Task",
            "Person",
            Some("Task is assigned to a person"),
        ),
        OntologyRelation::new(
            "PART_OF",
            "Task",
            "Project",
            Some("Task is part of a project"),
        ),
        OntologyRelation::new(
            "AFFECTS",
            "Decision",
            "Organization",
            Some("Decision affects an organization"),
        ),
        OntologyRelation::new(
            "DOCUMENTS",
            "Concept",
            "Project",
            Some("Concept provides context for a project"),
        ),
        OntologyRelation::new(
            "REPORTS_TO",
            "Person",
            "Person",
            Some("Person reports to another person"),
        ),
        OntologyRelation::new(
            "MANAGES",
            "Person",
            "Organization",
            Some("Person manages an organization"),
        ),
        OntologyRelation::new(
            "CONTAINS",
            "Organization",
            "Organization",
            Some("Organization contains sub-organizations"),
        ),
        OntologyRelation::new(
            "CREATED_BY",
            "Decision",
            "Person",
            Some("Decision was made by a person"),
        ),
    ]
}

/// Returns the 22 canonical world model properties.
pub fn canonical_world_model_properties() -> Vec<OntologyProperty> {
    vec![
        // Person properties
        OntologyProperty::required(
            "Person",
            OwnerKind::Class,
            "name",
            PropertyType::String,
            Some("Full name or primary identifier"),
        ),
        OntologyProperty::optional(
            "Person",
            OwnerKind::Class,
            "email",
            PropertyType::String,
            Some("Primary email address"),
        ),
        OntologyProperty::optional(
            "Person",
            OwnerKind::Class,
            "role_title",
            PropertyType::String,
            Some("Current job title or role"),
        ),
        OntologyProperty::optional(
            "Person",
            OwnerKind::Class,
            "bio",
            PropertyType::String,
            Some("Biography or description"),
        ),
        // Organization properties
        OntologyProperty::required(
            "Organization",
            OwnerKind::Class,
            "name",
            PropertyType::String,
            Some("Organization name"),
        ),
        OntologyProperty::optional(
            "Organization",
            OwnerKind::Class,
            "description",
            PropertyType::String,
            Some("Organization description and mission"),
        ),
        OntologyProperty::optional(
            "Organization",
            OwnerKind::Class,
            "founded_at",
            PropertyType::DateTime,
            Some("When the organization was founded"),
        ),
        // Project properties
        OntologyProperty::required(
            "Project",
            OwnerKind::Class,
            "name",
            PropertyType::String,
            Some("Project name"),
        ),
        OntologyProperty::optional(
            "Project",
            OwnerKind::Class,
            "description",
            PropertyType::String,
            Some("Project description and goals"),
        ),
        OntologyProperty::optional(
            "Project",
            OwnerKind::Class,
            "status",
            PropertyType::String,
            Some("Project status (active, paused, completed)"),
        ),
        OntologyProperty::optional(
            "Project",
            OwnerKind::Class,
            "start_date",
            PropertyType::DateTime,
            Some("Project start date"),
        ),
        OntologyProperty::optional(
            "Project",
            OwnerKind::Class,
            "target_date",
            PropertyType::DateTime,
            Some("Project target completion date"),
        ),
        // Task properties
        OntologyProperty::required(
            "Task",
            OwnerKind::Class,
            "name",
            PropertyType::String,
            Some("Task name or title"),
        ),
        OntologyProperty::optional(
            "Task",
            OwnerKind::Class,
            "description",
            PropertyType::String,
            Some("Task description and details"),
        ),
        OntologyProperty::optional(
            "Task",
            OwnerKind::Class,
            "status",
            PropertyType::String,
            Some("Task status (open, in_progress, done)"),
        ),
        OntologyProperty::optional(
            "Task",
            OwnerKind::Class,
            "priority",
            PropertyType::String,
            Some("Task priority (low, normal, high)"),
        ),
        // Role properties
        OntologyProperty::required(
            "Role",
            OwnerKind::Class,
            "name",
            PropertyType::String,
            Some("Role name"),
        ),
        OntologyProperty::optional(
            "Role",
            OwnerKind::Class,
            "description",
            PropertyType::String,
            Some("Role responsibilities and scope"),
        ),
        // Event properties
        OntologyProperty::required(
            "Event",
            OwnerKind::Class,
            "name",
            PropertyType::String,
            Some("Event name"),
        ),
        OntologyProperty::optional(
            "Event",
            OwnerKind::Class,
            "date",
            PropertyType::DateTime,
            Some("Event date and time"),
        ),
        // Decision and Policy properties (inheriting naming pattern)
        OntologyProperty::required(
            "Decision",
            OwnerKind::Class,
            "name",
            PropertyType::String,
            Some("Decision description"),
        ),
        OntologyProperty::required(
            "Policy",
            OwnerKind::Class,
            "name",
            PropertyType::String,
            Some("Policy name"),
        ),
    ]
}

/// Count the canonical classes, relations, and properties.
/// Used for verification in tests.
pub fn count_canonical_symbols() -> (usize, usize, usize) {
    (
        canonical_world_model().len(),
        canonical_world_model_relations().len(),
        canonical_world_model_properties().len(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_canonical_counts() {
        let (classes, relations, properties) = count_canonical_symbols();
        assert_eq!(classes, 10, "Canonical model should have exactly 10 classes");
        assert_eq!(relations, 19, "Canonical model should have exactly 19 relations");
        assert_eq!(
            properties, 22,
            "Canonical model should have exactly 22 properties"
        );
    }

    #[test]
    fn test_class_creation() {
        let class = OntologyClass::new("Test", Some("A test class"));
        assert_eq!(class.name, "Test");
        assert!(!class.symbol_id.is_empty());
        assert_eq!(class.description, Some("A test class".into()));
    }

    #[test]
    fn test_relation_creation() {
        let rel = OntologyRelation::new("TEST_REL", "Class1", "Class2", Some("Test relation"));
        assert_eq!(rel.name, "TEST_REL");
        assert_eq!(rel.domain, "Class1");
        assert_eq!(rel.range, "Class2");
    }
}
