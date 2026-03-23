use std::collections::HashMap;

use sparrowdb::GraphDb;
use sparrowdb_execution::Value;

use crate::error::SoError;
use crate::model::{AliasKind, OntologyProperty, PropertyType, PropertyValue};
use crate::namespace::{DOMAIN_REL, HAS_PROPERTY_REL, PROPERTY_LABEL, RANGE_REL};
use crate::resolution::{escape_cypher_string, resolve, ResolvedSymbol};

/// Provenance properties that callers ARE allowed to set.
const ALLOWED_SO_KEYS: &[&str] = &["__so_source_label", "__so_source_rel"];

// ── ValidationContext ─────────────────────────────────────────────────────────

pub struct ValidationContext<'a> {
    pub db: &'a GraphDb,
}

impl<'a> ValidationContext<'a> {
    pub fn new(db: &'a GraphDb) -> Self {
        Self { db }
    }

    /// Validate an entity label + property map for create or update.
    ///
    /// Rules:
    /// 1. Label resolves to a canonical class.
    /// 2. No `__so_` keys except the allowed provenance pair.
    /// 3. Every non-reserved key is declared on the class.
    /// 4. Every value matches its declared type.
    /// 5. On create: all required properties without defaults must be present.
    pub fn validate_entity(
        &self,
        label: &str,
        properties: &HashMap<String, PropertyValue>,
        is_create: bool,
    ) -> Result<ResolvedSymbol, SoError> {
        // Rule 1: label resolves
        let class = resolve(self.db, label, AliasKind::Class)?;

        // Rule 2: no __so_ keys (except allowed provenance keys)
        for key in properties.keys() {
            if key.starts_with("__so_") && !ALLOWED_SO_KEYS.contains(&key.as_str()) {
                return Err(SoError::ReservedProperty(key.clone()));
            }
        }

        // Rule 3 + 4: all non-reserved keys declared + type-checked
        // Build merged property set: own class properties + inherited from ancestor classes.
        // Child-declared property wins over ancestor on name collision (child overrides parent).
        let own_props = self.get_properties_for_class(&class.symbol_id)?;
        let inherited_props = self.get_inherited_properties(&class.canonical_name)?;

        // Merge: start with inherited, then overwrite with own (child wins)
        let mut merged: HashMap<String, OntologyProperty> = HashMap::new();
        for p in inherited_props {
            merged.entry(p.name.clone()).or_insert(p);
        }
        for p in own_props {
            merged.insert(p.name.clone(), p);
        }
        let declared: Vec<OntologyProperty> = merged.into_values().collect();

        if !properties.is_empty() && declared.is_empty() {
            return Err(SoError::UnseedeedClass {
                class_name: class.canonical_name.clone(),
            });
        }
        for (key, value) in properties {
            if key.starts_with("__so_") {
                continue;
            }
            let prop = declared
                .iter()
                .find(|p| p.name == *key)
                .ok_or_else(|| SoError::UnknownSymbol {
                    name: key.clone(),
                    kind: "property".to_string(),
                    valid: declared.iter().map(|p| p.name.clone()).collect(),
                    closest_match: None,
                    suggestion: None,
                })?;
            self.check_type_match(&class.canonical_name, prop, value)?;
        }

        // Rule 5: required properties must be present on create (including inherited)
        if is_create {
            for prop in &declared {
                if prop.required
                    && !properties.contains_key(&prop.name)
                    && prop.default_value.is_none()
                {
                    return Err(SoError::RequiredPropertyMissing {
                        class: class.canonical_name.clone(),
                        property: prop.name.clone(),
                    });
                }
            }
        }

        Ok(class)
    }

    /// Validate that `rel_type` is valid between `source_label` and `target_label`.
    ///
    /// Both label strings must be canonical or alias-resolvable.
    /// Domain and range checks are subclass-aware.
    ///
    /// Takes canonical label strings (not node IDs). MCP/CLI resolves IDs to labels first.
    pub fn validate_relationship(
        &self,
        rel_type: &str,
        source_label: &str,
        target_label: &str,
    ) -> Result<ResolvedSymbol, SoError> {
        let relation = resolve(self.db, rel_type, AliasKind::Relation)?;
        let source = resolve(self.db, source_label, AliasKind::Class)?;
        let domain = self.get_domain(&relation.symbol_id)?;

        if !self.is_subclass_of(&source.canonical_name, &domain)? {
            return Err(SoError::DomainViolation {
                relation: relation.canonical_name.clone(),
                expected: domain,
                actual: source.canonical_name.clone(),
            });
        }

        let target = resolve(self.db, target_label, AliasKind::Class)?;
        let range = self.get_range(&relation.symbol_id)?;

        if !self.is_subclass_of(&target.canonical_name, &range)? {
            return Err(SoError::RangeViolation {
                relation: relation.canonical_name.clone(),
                expected: range,
                actual: target.canonical_name.clone(),
            });
        }

        Ok(relation)
    }

    /// Return true if `class_name` == `ancestor_name` OR `class_name` transitively
    /// inherits from `ancestor_name` via `__SO_SUBCLASS_OF` (up to depth 20).
    pub fn is_subclass_of(
        &self,
        class_name: &str,
        ancestor_name: &str,
    ) -> Result<bool, SoError> {
        if class_name == ancestor_name {
            return Ok(true);
        }
        // BFS from class_name following SUBCLASS_OF edges toward ancestors.
        // Single-hop queries avoid SparrowDB's variable-length path bugs.
        use std::collections::HashSet;
        let mut visited: HashSet<String> = HashSet::new();
        let mut frontier: Vec<String> = vec![class_name.to_string()];

        while let Some(current) = frontier.pop() {
            if !visited.insert(current.clone()) {
                continue;
            }
            let safe_curr = escape_cypher_string(&current);
            let q = format!(
                "MATCH (c:__SO_Class)-[:__SO_SUBCLASS_OF]->(a:__SO_Class) \
                 WHERE c.name = '{safe_curr}' RETURN a.name"
            );
            let result = match self.db.execute(&q) {
                Ok(r) => r,
                Err(sparrowdb_common::Error::InvalidArgument(ref msg))
                    if msg.contains("unknown label")
                        || msg.contains("unknown relationship type") =>
                {
                    continue;
                }
                Err(e) => return Err(SoError::Storage(e)),
            };
            for row in &result.rows {
                if let Some(sparrowdb_execution::Value::String(name)) = row.first() {
                    if name == ancestor_name {
                        return Ok(true);
                    }
                    frontier.push(name.clone());
                }
            }
        }
        Ok(false)
    }

    /// Return all OntologyProperty definitions attached to the class with `symbol_id`.
    pub fn get_properties_for_class(
        &self,
        symbol_id: &str,
    ) -> Result<Vec<OntologyProperty>, SoError> {
        let safe_id = escape_cypher_string(symbol_id);
        let q = format!(
            "MATCH (c:__SO_Class {{symbol_id: '{safe_id}'}})-[:{HAS_PROPERTY_REL}]->(p:{PROPERTY_LABEL}) \
             RETURN p.symbol_id, p.name, p.datatype, p.required"
        );
        let result = self.db.execute(&q)?;
        let mut props = Vec::new();
        for row in &result.rows {
            if row.len() < 4 {
                continue;
            }
            let sym_id = match &row[0] {
                Value::String(s) => s.clone(),
                _ => continue,
            };
            let name = match &row[1] {
                Value::String(s) => s.clone(),
                _ => continue,
            };
            let datatype = parse_property_type(&row[2]);
            // required is stored as Int64(1/0) since storage has no Bool type
            let required = match &row[3] {
                Value::Bool(b) => *b,
                Value::Int64(n) => *n != 0,
                _ => false,
            };
            props.push(OntologyProperty {
                symbol_id: sym_id,
                name,
                datatype,
                required,
                default_value: None,
                owner_symbol_id: symbol_id.to_string(),
                owner_kind: crate::model::OwnerKind::Class,
                created_at: 0,
                owner_name: String::new(),
            });
        }
        Ok(props)
    }

    /// Walk the `__SO_SUBCLASS_OF` chain and return all properties declared on
    /// ancestor classes (not including the class itself). The caller is
    /// responsible for merging with own-class properties so that child
    /// declarations take precedence.
    pub fn get_inherited_properties(
        &self,
        class_name: &str,
    ) -> Result<Vec<OntologyProperty>, SoError> {
        use std::collections::HashSet;
        let mut all_props: Vec<OntologyProperty> = Vec::new();
        let mut visited: HashSet<String> = HashSet::new();
        let mut frontier: Vec<String> = vec![class_name.to_string()];

        // BFS up the hierarchy; skip the starting class itself (own props handled separately)
        visited.insert(class_name.to_string());

        while let Some(current) = frontier.pop() {
            // Find parents of current
            let safe_curr = escape_cypher_string(&current);
            let q = format!(
                "MATCH (c:__SO_Class)-[:__SO_SUBCLASS_OF]->(p:__SO_Class) \
                 WHERE c.name = '{safe_curr}' RETURN p.symbol_id, p.name"
            );
            let result = match self.db.execute(&q) {
                Ok(r) => r,
                Err(sparrowdb_common::Error::InvalidArgument(ref msg))
                    if msg.contains("unknown label")
                        || msg.contains("unknown relationship type") =>
                {
                    continue;
                }
                Err(e) => return Err(SoError::Storage(e)),
            };
            for row in &result.rows {
                let parent_sym_id = match row.first() {
                    Some(sparrowdb_execution::Value::String(s)) => s.clone(),
                    _ => continue,
                };
                let parent_name = match row.get(1) {
                    Some(sparrowdb_execution::Value::String(s)) => s.clone(),
                    _ => continue,
                };
                if visited.insert(parent_name.clone()) {
                    // Collect this parent's own properties
                    let parent_props = self.get_properties_for_class(&parent_sym_id)?;
                    all_props.extend(parent_props);
                    frontier.push(parent_name);
                }
            }
        }
        Ok(all_props)
    }

    /// Return the canonical domain class name for a relation.
    pub fn get_domain(&self, relation_symbol_id: &str) -> Result<String, SoError> {
        let safe = escape_cypher_string(relation_symbol_id);
        let q = format!(
            "MATCH (r:__SO_Relation {{symbol_id: '{safe}'}})-[:{DOMAIN_REL}]->(c:__SO_Class) \
             RETURN c.name"
        );
        let result = self.db.execute(&q)?;
        result
            .rows
            .first()
            .and_then(|r| r.first())
            .and_then(|v| {
                if let Value::String(s) = v {
                    Some(s.clone())
                } else {
                    None
                }
            })
            .ok_or_else(|| SoError::Storage(sparrowdb_common::Error::NotFound))
    }

    /// Return the canonical range class name for a relation.
    pub fn get_range(&self, relation_symbol_id: &str) -> Result<String, SoError> {
        let safe = escape_cypher_string(relation_symbol_id);
        let q = format!(
            "MATCH (r:__SO_Relation {{symbol_id: '{safe}'}})-[:{RANGE_REL}]->(c:__SO_Class) \
             RETURN c.name"
        );
        let result = self.db.execute(&q)?;
        result
            .rows
            .first()
            .and_then(|r| r.first())
            .and_then(|v| {
                if let Value::String(s) = v {
                    Some(s.clone())
                } else {
                    None
                }
            })
            .ok_or_else(|| SoError::Storage(sparrowdb_common::Error::NotFound))
    }

    /// Check that `value` matches the declared `prop.datatype`.
    pub fn check_type_match(
        &self,
        class_name: &str,
        prop: &OntologyProperty,
        value: &PropertyValue,
    ) -> Result<(), SoError> {
        let ok = match (&prop.datatype, value) {
            (PropertyType::String, PropertyValue::String(_)) => true,
            (PropertyType::Date, PropertyValue::String(_)) => true, // dates stored as strings in v1
            (PropertyType::Int64, PropertyValue::Int64(_)) => true,
            (PropertyType::Float64, PropertyValue::Float64(_)) => true,
            (PropertyType::Bool, PropertyValue::Bool(_)) => true,
            (PropertyType::Variant, _) => true, // variant accepts anything
            (_, PropertyValue::Null) => !prop.required, // null is ok for optional
            _ => false,
        };
        if !ok {
            return Err(SoError::TypeMismatch {
                class: class_name.to_string(),
                property: prop.name.clone(),
                expected: format!("{:?}", prop.datatype),
                actual: format!("{:?}", value),
            });
        }
        Ok(())
    }
}

// ── Full-graph validation scan (stubbed — SPA-209 required for db.labels()) ──

/// Full-graph validation report.
pub struct ValidationReport {
    pub violations: Vec<ValidationViolation>,
    pub warnings: Vec<ValidationWarning>,
    pub stats: ValidationStats,
}

pub struct ValidationViolation {
    pub kind: ViolationKind,
    pub node_id: Option<String>,
    pub edge_id: Option<String>,
    pub message: String,
    pub suggestion: Option<String>,
}

pub struct ValidationWarning {
    pub message: String,
}

pub enum ViolationKind {
    UnknownClass,
    UnknownRelationType,
    DomainViolation,
    RangeViolation,
    RequiredPropertyMissing,
    TypeMismatch,
    ReservedNamespaceCorruption,
}

pub struct ValidationStats {
    pub nodes_scanned: u64,
    pub edges_scanned: u64,
    pub violations_found: u64,
    pub warnings_found: u64,
    pub duration_ms: u64,
}

/// Full-graph validation scan.
///
/// TODO SPA-209: Full implementation requires `db.labels()` and
/// `db.relationship_types()` which are not yet available.
/// Returns an empty report (no violations) until SPA-209 ships.
pub fn validate(_db: &GraphDb) -> Result<ValidationReport, SoError> {
    Ok(ValidationReport {
        violations: Vec::new(),
        warnings: Vec::new(),
        stats: ValidationStats {
            nodes_scanned: 0,
            edges_scanned: 0,
            violations_found: 0,
            warnings_found: 0,
            duration_ms: 0,
        },
    })
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn parse_property_type(v: &Value) -> PropertyType {
    match v {
        Value::String(s) => match s.as_str() {
            "string" => PropertyType::String,
            "int64" => PropertyType::Int64,
            "float64" => PropertyType::Float64,
            "bool" => PropertyType::Bool,
            "date" => PropertyType::Date,
            "variant" => PropertyType::Variant,
            _ => PropertyType::Variant,
        },
        _ => PropertyType::Variant,
    }
}

// Re-export expand_subclasses for use in explain_symbol (Phase 2).
pub use crate::hierarchy::expand_subclasses;
