use std::collections::HashMap;

use sparrowdb::GraphDb;

use crate::error::SoError;
use crate::model::{AliasKind, PropertyType, ResolvedSymbol};
use crate::namespace::*;
use crate::resolution::{escape_cypher_string, resolve};

/// Context for validating entities and relationships against the ontology.
pub struct ValidationContext<'a> {
    db: &'a GraphDb,
}

impl<'a> ValidationContext<'a> {
    pub fn new(db: &'a GraphDb) -> Self {
        ValidationContext { db }
    }

    /// Validate an entity (node) against its class definition.
    ///
    /// Checks:
    /// 1. The label is a valid class (canonical or alias)
    /// 2. All required properties are present
    /// 3. All property values match their declared types
    /// 4. No `__so_*` properties are used outside the allowed whitelist
    ///
    /// # Arguments
    /// - `label`: The class name (canonical or alias)
    /// - `properties`: The entity's properties as a map
    /// - `is_create`: True if this is a CREATE operation (false for UPDATE)
    ///
    /// Returns the resolved class symbol if validation passes.
    pub fn validate_entity(
        &self,
        label: &str,
        properties: &HashMap<String, String>,
        is_create: bool,
    ) -> Result<ResolvedSymbol, SoError> {
        // Validate the label itself
        let class_symbol = resolve(self.db, label, AliasKind::Class)?;

        // Check for reserved property names
        for key in properties.keys() {
            if key.starts_with("__so_") && !ALLOWED_SO_PROPERTIES.contains(&key.as_str()) {
                return Err(SoError::ReservedProperty {
                    property: key.clone(),
                });
            }
        }

        // Get required properties for this class
        let required_props = self.get_required_properties_for_class(&class_symbol.canonical_name)?;

        // If creating, check that all required properties are present
        if is_create {
            for req_prop in &required_props {
                if !properties.contains_key(req_prop) {
                    return Err(SoError::required_property_missing(
                        &class_symbol.canonical_name,
                        req_prop,
                        required_props.clone(),
                    ));
                }
            }
        }

        // Type checking: validate each property value against its declared type
        for (prop_name, prop_value) in properties {
            if let Ok(prop_type) = self.get_property_datatype(&class_symbol.canonical_name, prop_name)
            {
                self.check_type_match(&prop_type, prop_value)?;
            }
        }

        Ok(class_symbol)
    }

    /// Validate a relationship (edge) against its definition.
    ///
    /// Checks:
    /// 1. The relation is a valid relation (canonical or alias)
    /// 2. The source node's class is in the domain
    /// 3. The target node's class is in the range
    pub fn validate_relationship(
        &self,
        rel_type: &str,
        source_label: &str,
        target_label: &str,
    ) -> Result<ResolvedSymbol, SoError> {
        // Validate the relation type
        let rel_symbol = resolve(self.db, rel_type, AliasKind::Relation)?;

        // Get domain and range for this relation
        let domain = self.get_domain(&rel_symbol.canonical_name)?;
        let range = self.get_range(&rel_symbol.canonical_name)?;

        // Resolve source and target labels
        let source_class = resolve(self.db, source_label, AliasKind::Class)?;
        let target_class = resolve(self.db, target_label, AliasKind::Class)?;

        // Check domain: source_label must be the domain or a subclass of it
        if source_class.canonical_name != domain {
            if !self.is_subclass_of(&source_class.canonical_name, &domain)? {
                return Err(SoError::domain_violation(
                    &rel_symbol.canonical_name,
                    &domain,
                    &source_class.canonical_name,
                    vec![domain.clone()],
                ));
            }
        }

        // Check range: target_label must be the range or a subclass of it
        if target_class.canonical_name != range {
            if !self.is_subclass_of(&target_class.canonical_name, &range)? {
                return Err(SoError::range_violation(
                    &rel_symbol.canonical_name,
                    &range,
                    &target_class.canonical_name,
                    vec![range.clone()],
                ));
            }
        }

        Ok(rel_symbol)
    }

    /// Check if `subclass_name` is a subclass (direct or transitive) of `ancestor_name`.
    pub fn is_subclass_of(&self, subclass_name: &str, ancestor_name: &str) -> Result<bool, SoError> {
        if subclass_name == ancestor_name {
            return Ok(true);
        }

        is_subclass_of(self.db, subclass_name, ancestor_name)
    }

    /// Get all required property names for a class.
    fn get_required_properties_for_class(&self, class_name: &str) -> Result<Vec<String>, SoError> {
        let escaped_class = escape_cypher_string(class_name);
        let query = format!(
            "MATCH (c:{} {{name: '{}'}}) <- [{}] - (p:{})
             WHERE p.is_required = true
             RETURN p.name",
            CLASS_LABEL, escaped_class, PROPERTY_OF, PROPERTY_LABEL
        );

        let result = self
            .db
            .execute(&query)
            .map_err(|e| SoError::Storage {
                message: e.to_string(),
            })?;

        let props: Vec<String> = result
            .rows
            .iter()
            .filter_map(|row| row.get(0).and_then(|v| if let sparrowdb_execution::Value::String(s) = v { Some(s.as_str()) } else { None }).map(|s| s.to_string()))
            .collect();

        Ok(props)
    }

    /// Get the datatype of a property on a class.
    fn get_property_datatype(&self, class_name: &str, prop_name: &str) -> Result<PropertyType, SoError> {
        let escaped_class = escape_cypher_string(class_name);
        let escaped_prop = escape_cypher_string(prop_name);

        let query = format!(
            "MATCH (c:{} {{name: '{}'}}) <- [{}] - (p:{} {{name: '{}'}})
             RETURN p.datatype",
            CLASS_LABEL, escaped_class, PROPERTY_OF, PROPERTY_LABEL, escaped_prop
        );

        let result = self
            .db
            .execute(&query)
            .map_err(|e| SoError::Storage {
                message: e.to_string(),
            })?;

        if result.rows.is_empty() {
            // Property not found for this class
            return Err(SoError::UnknownSymbol {
                symbol: prop_name.to_string(),
                kind: "Property".to_string(),
                valid_options: String::new(),
            });
        }

        let datatype_str = result.rows[0]
            .get(0)
            .and_then(|v| if let sparrowdb_execution::Value::String(s) = v { Some(s.as_str()) } else { None })
            .ok_or_else(|| SoError::Storage {
                message: "Property datatype is null".to_string(),
            })?;

        match datatype_str.as_ref() {
            "String" => Ok(PropertyType::String),
            "Integer" => Ok(PropertyType::Integer),
            "Float" => Ok(PropertyType::Float),
            "Boolean" => Ok(PropertyType::Boolean),
            "DateTime" => Ok(PropertyType::DateTime),
            "Json" => Ok(PropertyType::Json),
            _ => Err(SoError::Storage {
                message: format!("Unknown datatype: {}", datatype_str),
            }),
        }
    }

    /// Check if a property value matches its expected type.
    fn check_type_match(&self, prop_type: &PropertyType, value: &str) -> Result<(), SoError> {
        match prop_type {
            PropertyType::String => Ok(()),  // Any string is valid
            PropertyType::Integer => {
                value.parse::<i64>().map(|_| ()).map_err(|_| {
                    SoError::TypeMismatch {
                        property: "<unknown>".to_string(),
                        expected_type: "Integer".to_string(),
                        actual_type: "String".to_string(),
                        example_value: "123".to_string(),
                    }
                })
            }
            PropertyType::Float => {
                value.parse::<f64>().map(|_| ()).map_err(|_| {
                    SoError::TypeMismatch {
                        property: "<unknown>".to_string(),
                        expected_type: "Float".to_string(),
                        actual_type: "String".to_string(),
                        example_value: "3.14".to_string(),
                    }
                })
            }
            PropertyType::Boolean => {
                if ["true", "false"].contains(&value.to_lowercase().as_str()) {
                    Ok(())
                } else {
                    Err(SoError::TypeMismatch {
                        property: "<unknown>".to_string(),
                        expected_type: "Boolean".to_string(),
                        actual_type: "String".to_string(),
                        example_value: "true".to_string(),
                    })
                }
            }
            PropertyType::DateTime => {
                // Basic ISO 8601 validation
                if chrono::DateTime::parse_from_rfc3339(value).is_ok()
                    || chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d").is_ok()
                {
                    Ok(())
                } else {
                    Err(SoError::TypeMismatch {
                        property: "<unknown>".to_string(),
                        expected_type: "DateTime".to_string(),
                        actual_type: "String".to_string(),
                        example_value: "2026-03-22".to_string(),
                    })
                }
            }
            PropertyType::Json => {
                serde_json::from_str::<serde_json::Value>(value)
                    .map(|_| ())
                    .map_err(|_| SoError::TypeMismatch {
                        property: "<unknown>".to_string(),
                        expected_type: "Json".to_string(),
                        actual_type: "String".to_string(),
                        example_value: r#"{"key": "value"}"#.to_string(),
                    })
            }
        }
    }

    /// Get the domain class for a relation.
    fn get_domain(&self, rel_name: &str) -> Result<String, SoError> {
        let escaped_rel = escape_cypher_string(rel_name);
        let query = format!(
            "MATCH (r:{} {{name: '{}'}}) - [{}] -> (d:{})
             RETURN d.name",
            RELATION_LABEL, escaped_rel, DOMAIN, CLASS_LABEL
        );

        let result = self
            .db
            .execute(&query)
            .map_err(|e| SoError::Storage {
                message: e.to_string(),
            })?;

        if result.rows.is_empty() {
            return Err(SoError::UnknownSymbol {
                symbol: rel_name.to_string(),
                kind: "Relation".to_string(),
                valid_options: String::new(),
            });
        }

        result.rows[0]
            .get(0)
            .and_then(|v| if let sparrowdb_execution::Value::String(s) = v { Some(s.as_str()) } else { None })
            .map(|s| s.to_string())
            .ok_or_else(|| SoError::Storage {
                message: "Domain is null".to_string(),
            })
    }

    /// Get the range class for a relation.
    fn get_range(&self, rel_name: &str) -> Result<String, SoError> {
        let escaped_rel = escape_cypher_string(rel_name);
        let query = format!(
            "MATCH (r:{} {{name: '{}'}}) - [{}] -> (rng:{})
             RETURN rng.name",
            RELATION_LABEL, escaped_rel, RANGE, CLASS_LABEL
        );

        let result = self
            .db
            .execute(&query)
            .map_err(|e| SoError::Storage {
                message: e.to_string(),
            })?;

        if result.rows.is_empty() {
            return Err(SoError::UnknownSymbol {
                symbol: rel_name.to_string(),
                kind: "Relation".to_string(),
                valid_options: String::new(),
            });
        }

        result.rows[0]
            .get(0)
            .and_then(|v| if let sparrowdb_execution::Value::String(s) = v { Some(s.as_str()) } else { None })
            .map(|s| s.to_string())
            .ok_or_else(|| SoError::Storage {
                message: "Range is null".to_string(),
            })
    }
}

/// Check if class `child` is a subclass of class `parent`.
pub fn is_subclass_of(db: &GraphDb, child: &str, parent: &str) -> Result<bool, SoError> {
    if child == parent {
        return Ok(true);
    }

    let escaped_child = escape_cypher_string(child);
    let escaped_parent = escape_cypher_string(parent);

    let query = format!(
        "MATCH (c:{} {{name: '{}'}}) - [*1..] -> (p:{} {{name: '{}'}})
         RETURN count(*) > 0",
        CLASS_LABEL, escaped_child, CLASS_LABEL, escaped_parent
    );

    let result = db
        .execute(&query)
        .map_err(|e| SoError::Storage {
            message: e.to_string(),
        })?;

    if result.rows.is_empty() {
        return Ok(false);
    }

    result.rows[0]
        .get(0)
        .and_then(|v| if let sparrowdb_execution::Value::Bool(b) = v { Some(*b) } else { None })
        .ok_or_else(|| SoError::Storage {
            message: "Query result is invalid".to_string(),
        })
}
