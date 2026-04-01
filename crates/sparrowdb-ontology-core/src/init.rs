use serde::{Deserialize, Serialize};
use std::str::FromStr;

use sparrowdb::GraphDb;

use crate::error::SoError;
use crate::model::*;
use crate::namespace::*;
use crate::resolution::escape_cypher_string;

/// Result of initializing the ontology.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitResult {
    pub classes_created: usize,
    pub relations_created: usize,
    pub properties_created: usize,
}

/// Initialize the ontology in the given SparrowDB instance.
///
/// If the ontology is already initialized (detected by checking for existing __SO_Class nodes):
/// - If `force=false`, returns AlreadyInitialized error
/// - If `force=true`, wipes all __SO_* nodes and reinitializes
///
/// Seeds the canonical world model: 10 classes, 19 relations, 22 properties.
pub fn init(db: &GraphDb, force: bool) -> Result<InitResult, SoError> {
    // Check if already initialized
    let check_query = format!("MATCH (n:{}) RETURN count(n) as count", CLASS_LABEL);
    let check_result = db
        .execute(&check_query)
        .map_err(|e| SoError::Storage {
            message: e.to_string(),
        })?;

    let existing_count = check_result.rows[0]
        .get(0)
        .and_then(|v| if let sparrowdb_execution::Value::Int64(i) = v { Some(*i) } else { None })
        .unwrap_or(0) as usize;

    if existing_count > 0 && !force {
        // Already initialized — get counts for error message
        let rel_query = format!("MATCH (n:{}) RETURN count(n) as count", RELATION_LABEL);
        let rel_result = db
            .execute(&rel_query)
            .map_err(|e| SoError::Storage {
                message: e.to_string(),
            })?;
        let rel_count = rel_result.rows[0]
            .get(0)
            .and_then(|v| if let sparrowdb_execution::Value::Int64(i) = v { Some(*i) } else { None })
            .unwrap_or(0) as usize;

        let prop_query = format!("MATCH (n:{}) RETURN count(n) as count", PROPERTY_LABEL);
        let prop_result = db
            .execute(&prop_query)
            .map_err(|e| SoError::Storage {
                message: e.to_string(),
            })?;
        let prop_count = prop_result.rows[0]
            .get(0)
            .and_then(|v| if let sparrowdb_execution::Value::Int64(i) = v { Some(*i) } else { None })
            .unwrap_or(0) as usize;

        return Err(SoError::AlreadyInitialized {
            class_count: existing_count,
            relation_count: rel_count,
            property_count: prop_count,
        });
    }

    // If force=true, wipe all existing __SO_* nodes
    if force && existing_count > 0 {
        for label in RESERVED_LABELS {
            let delete_query = format!("MATCH (n:{}) DETACH DELETE n", label);
            db.execute(&delete_query).map_err(|e| SoError::Storage {
                message: e.to_string(),
            })?;
        }
    }

    // Get canonical world model
    let classes = canonical_world_model();
    let relations = canonical_world_model_relations();
    let properties = canonical_world_model_properties();

    // Seed classes
    for class in &classes {
        seed_class(db, class)?;
    }

    // Seed relations with their domain and range edges
    for relation in &relations {
        seed_relation(db, relation)?;
    }

    // Seed properties
    for property in &properties {
        seed_property(db, property)?;
    }

    Ok(InitResult {
        classes_created: classes.len(),
        relations_created: relations.len(),
        properties_created: properties.len(),
    })
}

/// Create a __SO_Class node for the given class.
fn seed_class(db: &GraphDb, class: &OntologyClass) -> Result<(), SoError> {
    let escaped_name = escape_cypher_string(&class.name);
    let description_clause = if let Some(desc) = &class.description {
        let escaped_desc = escape_cypher_string(desc);
        format!(", description: '{}'", escaped_desc)
    } else {
        String::new()
    };

    let create_query = format!(
        "CREATE (c:{} {{name: '{}', symbol_id: '{}', status: 'Active'{}}})",
        CLASS_LABEL, escaped_name, class.symbol_id, description_clause
    );

    db.execute(&create_query)
        .map_err(|e| SoError::Storage {
            message: e.to_string(),
        })?;

    Ok(())
}

/// Create a __SO_Relation node and its domain/range edges.
fn seed_relation(db: &GraphDb, relation: &OntologyRelation) -> Result<(), SoError> {
    let escaped_name = escape_cypher_string(&relation.name);
    let escaped_domain = escape_cypher_string(&relation.domain);
    let escaped_range = escape_cypher_string(&relation.range);

    let description_clause = if let Some(desc) = &relation.description {
        let escaped_desc = escape_cypher_string(desc);
        format!(", description: '{}'", escaped_desc)
    } else {
        String::new()
    };

    // Create the relation node
    let rel_query = format!(
        "CREATE (r:{} {{name: '{}', symbol_id: '{}', status: 'Active'{}}})",
        RELATION_LABEL, escaped_name, relation.symbol_id, description_clause
    );

    db.execute(&rel_query).map_err(|e| SoError::Storage {
        message: e.to_string(),
    })?;

    // Create DOMAIN edge to domain class
    let domain_query = format!(
        "MATCH (r:{} {{name: '{}'}}) , (d:{} {{name: '{}'}})
         CREATE (r) - [:{} ] -> (d)",
        RELATION_LABEL, escaped_name, CLASS_LABEL, escaped_domain, DOMAIN
    );

    db.execute(&domain_query).map_err(|e| SoError::Storage {
        message: e.to_string(),
    })?;

    // Create RANGE edge to range class
    let range_query = format!(
        "MATCH (r:{} {{name: '{}'}}) , (rng:{} {{name: '{}'}})
         CREATE (r) - [:{} ] -> (rng)",
        RELATION_LABEL, escaped_name, CLASS_LABEL, escaped_range, RANGE
    );

    db.execute(&range_query).map_err(|e| SoError::Storage {
        message: e.to_string(),
    })?;

    Ok(())
}

/// Create a __SO_Property node and link it to its owner.
fn seed_property(db: &GraphDb, property: &OntologyProperty) -> Result<(), SoError> {
    let escaped_name = escape_cypher_string(&property.name);
    let escaped_owner = escape_cypher_string(&property.owner);
    let datatype_str = property.datatype.to_string();

    let description_clause = if let Some(desc) = &property.description {
        let escaped_desc = escape_cypher_string(desc);
        format!(", description: '{}'", escaped_desc)
    } else {
        String::new()
    };

    let source_iri_clause = if let Some(iri) = &property.source_iri {
        let escaped_iri = escape_cypher_string(iri);
        format!(", source_iri: '{}'", escaped_iri)
    } else {
        String::new()
    };

    let create_query = format!(
        "CREATE (p:{} {{name: '{}', symbol_id: '{}', datatype: '{}', is_required: {}, status: 'Active'{}{}}})",
        PROPERTY_LABEL,
        escaped_name,
        property.symbol_id,
        datatype_str,
        if property.is_required { "true" } else { "false" },
        description_clause,
        source_iri_clause
    );

    db.execute(&create_query).map_err(|e| SoError::Storage {
        message: e.to_string(),
    })?;

    // Determine owner label (Class or Relation)
    let owner_label = match property.owner_kind {
        OwnerKind::Class => CLASS_LABEL,
        OwnerKind::Relation => RELATION_LABEL,
    };

    // Create PROPERTY_OF edge from property to owner
    let link_query = format!(
        "MATCH (p:{} {{name: '{}'}}) , (o:{} {{name: '{}'}})
         CREATE (p) - [:{} ] -> (o)",
        PROPERTY_LABEL, escaped_name, owner_label, escaped_owner, PROPERTY_OF
    );

    db.execute(&link_query).map_err(|e| SoError::Storage {
        message: e.to_string(),
    })?;

    Ok(())
}

/// Add a new `__SO_Property` to an existing owner class or relation.
///
/// This allows callers (e.g. a Turtle importer) to persist properties discovered
/// from external ontologies without calling `init` again.
pub fn add_property(
    db: &GraphDb,
    owner: &str,
    owner_kind: OwnerKind,
    prop_name: &str,
    datatype_str: &str,
    required: bool,
    description: Option<&str>,
    source_iri: Option<&str>,
) -> Result<OntologyProperty, SoError> {
    let datatype = PropertyType::from_str(datatype_str)?;
    let prop = if required {
        OntologyProperty::required(owner, owner_kind, prop_name, datatype, description, source_iri)
    } else {
        OntologyProperty::optional(owner, owner_kind, prop_name, datatype, description, source_iri)
    };
    seed_property(db, &prop)?;
    Ok(prop)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init_result_counts() {
        let result = InitResult {
            classes_created: 10,
            relations_created: 19,
            properties_created: 22,
        };
        assert_eq!(result.classes_created, 10);
        assert_eq!(result.relations_created, 19);
        assert_eq!(result.properties_created, 22);
    }
}
