use serde::{Deserialize, Serialize};

// ── Timestamp helper ─────────────────────────────────────────────────────────

fn now_utc_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before Unix epoch")
        .as_millis() as i64
}

// ── Enums ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SymbolStatus {
    Active,
    Deprecated,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AliasKind {
    Class,
    Relation,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PropertyType {
    String,
    Int64,
    Float64,
    Bool,
    Date,
    Variant,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PropertyValue {
    String(String),
    Int64(i64),
    Float64(f64),
    Bool(bool),
    Null,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum OwnerKind {
    Class,
    Relation,
}

// ── Structs ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OntologyClass {
    pub symbol_id: String,
    pub name: String,
    pub description: Option<String>,
    pub status: SymbolStatus,
    pub created_at: i64,
    pub updated_at: i64,
}

impl OntologyClass {
    /// Create a new active class with a generated UUID v4 symbol_id
    /// and UTC Unix millisecond timestamps.
    pub fn new(name: &str, description: &str) -> Self {
        Self {
            symbol_id: uuid::Uuid::new_v4().to_string(),
            name: name.to_string(),
            description: Some(description.to_string()),
            status: SymbolStatus::Active,
            created_at: now_utc_ms(),
            updated_at: now_utc_ms(),
        }
    }
}

/// Note: `domain` and `range` are strings in this struct for API convenience.
/// In storage they are edges:
///   (:__SO_Relation)-[:__SO_DOMAIN]->(:__SO_Class)
///   (:__SO_Relation)-[:__SO_RANGE]->(:__SO_Class)
/// seed_relation() (§9.2) creates both the node and these edges.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OntologyRelation {
    pub symbol_id: String,
    pub name: String,
    pub description: Option<String>,
    pub status: SymbolStatus,
    pub domain: String,
    pub range: String,
    pub directed: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

impl OntologyRelation {
    pub fn new(name: &str, domain: &str, range: &str) -> Self {
        Self {
            symbol_id: uuid::Uuid::new_v4().to_string(),
            name: name.to_string(),
            description: None,
            status: SymbolStatus::Active,
            domain: domain.to_string(),
            range: range.to_string(),
            directed: true,
            created_at: now_utc_ms(),
            updated_at: now_utc_ms(),
        }
    }
}

/// Stored as (:__SO_Alias)-[:__SO_ALIAS_OF]->(:__SO_Class|:__SO_Relation)
/// Aliases carry spelling variants, not meaning.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OntologyAlias {
    pub name: String,
    pub kind: AliasKind,
    pub target_symbol_id: String,
    pub target_name: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OntologyProperty {
    pub symbol_id: String,
    pub name: String,
    pub datatype: PropertyType,
    pub required: bool,
    /// If true, a `CREATE CONSTRAINT ON (n:OwnerClass) ASSERT n.name IS UNIQUE`
    /// was emitted when this property was declared.
    pub unique: bool,
    /// If set, values must be one of these strings (enum enforcement).
    pub allowed_values: Option<Vec<String>>,
    pub default_value: Option<PropertyValue>,
    pub owner_symbol_id: String,
    pub owner_kind: OwnerKind,
    pub created_at: i64,
    /// Owner name — used during seeding to look up owner_symbol_id.
    pub owner_name: String,
}

impl OntologyProperty {
    pub fn required(owner: &str, name: &str, datatype: PropertyType) -> Self {
        Self {
            symbol_id: uuid::Uuid::new_v4().to_string(),
            name: name.to_string(),
            datatype,
            required: true,
            unique: false,
            allowed_values: None,
            default_value: None,
            owner_symbol_id: String::new(), // resolved during seed
            owner_kind: OwnerKind::Class,
            created_at: now_utc_ms(),
            owner_name: owner.to_string(),
        }
    }

    pub fn optional(owner: &str, name: &str, datatype: PropertyType) -> Self {
        Self {
            required: false,
            ..Self::required(owner, name, datatype)
        }
    }
}

// ── Canonical world model ─────────────────────────────────────────────────────

pub fn canonical_world_model() -> Vec<OntologyClass> {
    vec![
        OntologyClass::new("Person", "A human individual"),
        OntologyClass::new("Organization", "A company, team, institution, or group"),
        OntologyClass::new("Project", "A bounded piece of work with a goal"),
        OntologyClass::new("Task", "A discrete unit of work within a project"),
        OntologyClass::new("Role", "A function or position a person plays"),
        OntologyClass::new("Event", "A time-bounded occurrence"),
        OntologyClass::new("Decision", "A choice made, with context and rationale"),
        OntologyClass::new("Policy", "A rule, ethic, or operating principle"),
        OntologyClass::new("Concept", "An idea, skill, domain, or area of knowledge"),
        OntologyClass::new(
            "Dependency",
            "A reified dependency — carries metadata (reason, severity, resolution plan) \
             that bare DEPENDS_ON/BLOCKS edges cannot. Use DEPENDS_ON/BLOCKS for simple \
             blocking; use a Dependency node when the dependency itself needs tracking.",
        ),
    ]
}

pub fn canonical_world_model_relations() -> Vec<OntologyRelation> {
    vec![
        OntologyRelation::new("WORKS_FOR", "Person", "Organization"),
        OntologyRelation::new("WORKS_WITH", "Person", "Person"),
        OntologyRelation::new("BELONGS_TO", "Person", "Organization"),
        OntologyRelation::new("FOUNDED", "Person", "Organization"),
        OntologyRelation::new("OWNS", "Person", "Project"),
        OntologyRelation::new("PARTICIPATES_IN", "Person", "Project"),
        OntologyRelation::new("ASSIGNED_TO", "Person", "Task"),
        OntologyRelation::new("KNOWS", "Person", "Person"),
        OntologyRelation::new("REPORTS_TO", "Person", "Person"),
        OntologyRelation::new("TRUSTS", "Person", "Person"),
        OntologyRelation::new("CONTAINS", "Project", "Task"),
        OntologyRelation::new("DEPENDS_ON", "Task", "Task"),
        OntologyRelation::new("BLOCKS", "Task", "Task"),
        OntologyRelation::new("DECIDED", "Person", "Decision"),
        OntologyRelation::new("FOLLOWS", "Person", "Policy"),
        OntologyRelation::new("APPLIES_TO", "Policy", "Project"),
        OntologyRelation::new("KNOWS_CONCEPT", "Person", "Concept"),
        OntologyRelation::new("RELATED_TO", "Concept", "Concept"),
        OntologyRelation::new("HAS_DEPENDENCY", "Project", "Dependency"),
    ]
}

pub fn canonical_world_model_properties() -> Vec<OntologyProperty> {
    vec![
        OntologyProperty::required("Person", "name", PropertyType::String),
        OntologyProperty::optional("Person", "email", PropertyType::String),
        OntologyProperty::required("Organization", "name", PropertyType::String),
        OntologyProperty::optional("Organization", "description", PropertyType::String),
        OntologyProperty::required("Project", "name", PropertyType::String),
        OntologyProperty::optional("Project", "status", PropertyType::String),
        OntologyProperty::optional("Project", "startDate", PropertyType::Date),
        OntologyProperty::required("Task", "name", PropertyType::String),
        OntologyProperty::optional("Task", "status", PropertyType::String),
        OntologyProperty::optional("Task", "priority", PropertyType::String),
        OntologyProperty::required("Role", "name", PropertyType::String),
        OntologyProperty::required("Event", "name", PropertyType::String),
        OntologyProperty::optional("Event", "date", PropertyType::Date),
        OntologyProperty::required("Decision", "name", PropertyType::String),
        OntologyProperty::optional("Decision", "rationale", PropertyType::String),
        OntologyProperty::optional("Decision", "date", PropertyType::Date),
        OntologyProperty::required("Policy", "name", PropertyType::String),
        OntologyProperty::optional("Policy", "description", PropertyType::String),
        OntologyProperty::required("Concept", "name", PropertyType::String),
        OntologyProperty::required("Dependency", "name", PropertyType::String),
        OntologyProperty::optional("Dependency", "reason", PropertyType::String),
        OntologyProperty::optional("Dependency", "severity", PropertyType::String),
    ]
}

// ── PersonalKnowledge starter ─────────────────────────────────────────────────

pub fn personal_knowledge_classes() -> Vec<OntologyClass> {
    vec![
        OntologyClass::new("Person", "A human individual"),
        OntologyClass::new("Concept", "An idea, skill, domain, or area of knowledge"),
        OntologyClass::new("Event", "A time-bounded occurrence"),
        OntologyClass::new("Location", "A physical or virtual place"),
        OntologyClass::new("Document", "A file, note, article, or written artifact"),
    ]
}

pub fn personal_knowledge_relations() -> Vec<OntologyRelation> {
    vec![
        OntologyRelation::new("KNOWS", "Person", "Person"),
        OntologyRelation::new("RELATED_TO", "Concept", "Concept"),
        OntologyRelation::new("OCCURRED_AT", "Event", "Location"),
        OntologyRelation::new("LOCATED_IN", "Person", "Location"),
        OntologyRelation::new("PART_OF", "Concept", "Concept"),
    ]
}

pub fn personal_knowledge_properties() -> Vec<OntologyProperty> {
    vec![
        OntologyProperty::required("Person", "name", PropertyType::String),
        OntologyProperty::optional("Person", "description", PropertyType::String),
        OntologyProperty::optional("Person", "url", PropertyType::String),
        OntologyProperty::optional("Person", "created_at", PropertyType::Date),
        OntologyProperty::required("Concept", "name", PropertyType::String),
        OntologyProperty::optional("Concept", "description", PropertyType::String),
        OntologyProperty::optional("Concept", "created_at", PropertyType::Date),
        OntologyProperty::required("Event", "name", PropertyType::String),
        OntologyProperty::optional("Event", "description", PropertyType::String),
        OntologyProperty::optional("Event", "start_date", PropertyType::Date),
        OntologyProperty::optional("Event", "end_date", PropertyType::Date),
        OntologyProperty::optional("Event", "created_at", PropertyType::Date),
        OntologyProperty::required("Location", "name", PropertyType::String),
        OntologyProperty::optional("Location", "description", PropertyType::String),
        OntologyProperty::optional("Location", "created_at", PropertyType::Date),
        OntologyProperty::required("Document", "name", PropertyType::String),
        OntologyProperty::optional("Document", "description", PropertyType::String),
        OntologyProperty::optional("Document", "url", PropertyType::String),
        OntologyProperty::optional("Document", "created_at", PropertyType::Date),
    ]
}

// ── ProfessionalNetwork starter ───────────────────────────────────────────────

pub fn professional_network_classes() -> Vec<OntologyClass> {
    vec![
        OntologyClass::new("Person", "A human individual"),
        OntologyClass::new("Organization", "A company, team, institution, or group"),
        OntologyClass::new("Role", "A function or position a person plays"),
        OntologyClass::new("Project", "A bounded piece of work with a goal"),
        OntologyClass::new("Event", "A time-bounded occurrence"),
    ]
}

pub fn professional_network_relations() -> Vec<OntologyRelation> {
    vec![
        OntologyRelation::new("WORKS_FOR", "Person", "Organization"),
        OntologyRelation::new("MEMBER_OF", "Person", "Organization"),
        OntologyRelation::new("LEADS", "Person", "Project"),
        OntologyRelation::new("HAS_ROLE", "Person", "Role"),
        OntologyRelation::new("PARTICIPATED_IN", "Person", "Event"),
        OntologyRelation::new("DEPENDS_ON", "Project", "Project"),
    ]
}

pub fn professional_network_properties() -> Vec<OntologyProperty> {
    vec![
        OntologyProperty::required("Person", "name", PropertyType::String),
        OntologyProperty::optional("Person", "description", PropertyType::String),
        OntologyProperty::optional("Person", "email", PropertyType::String),
        OntologyProperty::optional("Person", "phone", PropertyType::String),
        OntologyProperty::optional("Person", "created_at", PropertyType::Date),
        OntologyProperty::required("Organization", "name", PropertyType::String),
        OntologyProperty::optional("Organization", "description", PropertyType::String),
        OntologyProperty::optional("Organization", "url", PropertyType::String),
        OntologyProperty::optional("Organization", "created_at", PropertyType::Date),
        OntologyProperty::required("Role", "name", PropertyType::String),
        OntologyProperty::optional("Role", "description", PropertyType::String),
        OntologyProperty::optional("Role", "created_at", PropertyType::Date),
        OntologyProperty::required("Project", "name", PropertyType::String),
        OntologyProperty::optional("Project", "description", PropertyType::String),
        OntologyProperty::optional("Project", "start_date", PropertyType::Date),
        OntologyProperty::optional("Project", "end_date", PropertyType::Date),
        OntologyProperty::optional("Project", "created_at", PropertyType::Date),
        OntologyProperty::required("Event", "name", PropertyType::String),
        OntologyProperty::optional("Event", "description", PropertyType::String),
        OntologyProperty::optional("Event", "start_date", PropertyType::Date),
        OntologyProperty::optional("Event", "end_date", PropertyType::Date),
        OntologyProperty::optional("Event", "created_at", PropertyType::Date),
    ]
}

// ── ResearchNotes starter ─────────────────────────────────────────────────────

pub fn research_notes_classes() -> Vec<OntologyClass> {
    vec![
        OntologyClass::new("Concept", "An idea, skill, domain, or area of knowledge"),
        OntologyClass::new("Document", "A file, note, article, or written artifact"),
        OntologyClass::new("Claim", "An assertion or proposition that can be supported or contradicted"),
        OntologyClass::new("Person", "A human individual"),
        OntologyClass::new("Asset", "A resource, dataset, tool, or artifact"),
    ]
}

pub fn research_notes_relations() -> Vec<OntologyRelation> {
    vec![
        OntologyRelation::new("CITES", "Document", "Document"),
        OntologyRelation::new("SUPPORTS", "Claim", "Claim"),
        OntologyRelation::new("CONTRADICTS", "Claim", "Claim"),
        OntologyRelation::new("DERIVED_FROM", "Concept", "Concept"),
        OntologyRelation::new("AUTHORED", "Person", "Document"),
        OntologyRelation::new("TAGGED_WITH", "Document", "Concept"),
    ]
}

pub fn research_notes_properties() -> Vec<OntologyProperty> {
    vec![
        OntologyProperty::required("Concept", "name", PropertyType::String),
        OntologyProperty::optional("Concept", "description", PropertyType::String),
        OntologyProperty::optional("Concept", "created_at", PropertyType::Date),
        OntologyProperty::required("Document", "name", PropertyType::String),
        OntologyProperty::optional("Document", "description", PropertyType::String),
        OntologyProperty::optional("Document", "url", PropertyType::String),
        OntologyProperty::optional("Document", "created_at", PropertyType::Date),
        OntologyProperty::required("Claim", "name", PropertyType::String),
        OntologyProperty::optional("Claim", "description", PropertyType::String),
        OntologyProperty::optional("Claim", "source", PropertyType::String),
        OntologyProperty::optional("Claim", "confidence", PropertyType::String),
        OntologyProperty::optional("Claim", "created_at", PropertyType::Date),
        OntologyProperty::required("Person", "name", PropertyType::String),
        OntologyProperty::optional("Person", "description", PropertyType::String),
        OntologyProperty::optional("Person", "created_at", PropertyType::Date),
        OntologyProperty::required("Asset", "name", PropertyType::String),
        OntologyProperty::optional("Asset", "description", PropertyType::String),
        OntologyProperty::optional("Asset", "url", PropertyType::String),
        OntologyProperty::optional("Asset", "created_at", PropertyType::Date),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_world_model_counts() {
        assert_eq!(canonical_world_model().len(), 10);
        assert_eq!(canonical_world_model_relations().len(), 19);
        assert_eq!(canonical_world_model_properties().len(), 22);
    }

    #[test]
    fn personal_knowledge_counts() {
        assert_eq!(personal_knowledge_classes().len(), 5);
        assert_eq!(personal_knowledge_relations().len(), 5);
        assert_eq!(personal_knowledge_properties().len(), 19);
    }

    #[test]
    fn professional_network_counts() {
        assert_eq!(professional_network_classes().len(), 5);
        assert_eq!(professional_network_relations().len(), 6);
        assert_eq!(professional_network_properties().len(), 22);
    }

    #[test]
    fn research_notes_counts() {
        assert_eq!(research_notes_classes().len(), 5);
        assert_eq!(research_notes_relations().len(), 6);
        assert_eq!(research_notes_properties().len(), 19);
    }
}
