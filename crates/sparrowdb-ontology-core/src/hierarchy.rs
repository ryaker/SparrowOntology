use sparrowdb::GraphDb;
use sparrowdb_execution::Value;

use crate::error::SoError;
use crate::namespace::{CLASS_LABEL, RELATION_LABEL, SUBCLASS_OF_REL, SUBPROPERTY_OF_REL};
use crate::resolution::escape_cypher_string;

/// Returns `class_name` itself plus all direct and transitive subclasses
/// (nodes that point TO `class_name` via `__SO_SUBCLASS_OF`, up to `depth` hops).
pub fn expand_subclasses(
    db: &GraphDb,
    class_name: &str,
    depth: usize,
) -> Result<Vec<String>, SoError> {
    let safe = escape_cypher_string(class_name);
    let q = format!(
        "MATCH (sub:{CLASS_LABEL})-[:{SUBCLASS_OF_REL}*1..{depth}]->(base:{CLASS_LABEL} {{name: '{safe}'}}) \
         RETURN sub.name"
    );
    let mut names = vec![class_name.to_string()];
    let result = match db.execute(&q) {
        Ok(r) => r,
        Err(sparrowdb_common::Error::InvalidArgument(ref msg))
            if msg.contains("unknown label") || msg.contains("unknown relationship type") =>
        {
            return Ok(names);
        }
        Err(e) => return Err(SoError::Storage(e)),
    };
    for row in &result.rows {
        if let Some(Value::String(s)) = row.first() {
            names.push(s.clone());
        }
    }
    Ok(names)
}

/// Returns `rel_name` itself plus all sub-relations (via `__SO_SUBPROPERTY_OF`,
/// up to `depth` hops).
pub fn expand_subproperties(
    db: &GraphDb,
    rel_name: &str,
    depth: usize,
) -> Result<Vec<String>, SoError> {
    let safe = escape_cypher_string(rel_name);
    let q = format!(
        "MATCH (sub:{RELATION_LABEL})-[:{SUBPROPERTY_OF_REL}*1..{depth}]->(base:{RELATION_LABEL} {{name: '{safe}'}}) \
         RETURN sub.name"
    );
    let mut names = vec![rel_name.to_string()];
    let result = match db.execute(&q) {
        Ok(r) => r,
        Err(sparrowdb_common::Error::InvalidArgument(ref msg))
            if msg.contains("unknown label") || msg.contains("unknown relationship type") =>
        {
            return Ok(names);
        }
        Err(e) => return Err(SoError::Storage(e)),
    };
    for row in &result.rows {
        if let Some(Value::String(s)) = row.first() {
            names.push(s.clone());
        }
    }
    Ok(names)
}

/// Check that adding `child → parent` via `edge_type` would NOT create a cycle.
///
/// A cycle exists if `parent` already (transitively) reaches `child` via `edge_type`.
/// Returns `SoError::CycleDetected` if a cycle would be formed.
pub fn check_no_cycle(
    db: &GraphDb,
    child: &str,
    parent: &str,
    edge_type: &str,
) -> Result<(), SoError> {
    let safe_parent = escape_cypher_string(parent);
    let safe_child = escape_cypher_string(child);
    let q = format!(
        "MATCH (p:{CLASS_LABEL} {{name: '{safe_parent}'}})-[:{edge_type}*1..50]->(c:{CLASS_LABEL} {{name: '{safe_child}'}}) \
         RETURN p.name LIMIT 1"
    );
    let result = match db.execute(&q) {
        Ok(r) => r,
        Err(sparrowdb_common::Error::InvalidArgument(ref msg))
            if msg.contains("unknown label") || msg.contains("unknown relationship type") =>
        {
            return Ok(());
        }
        Err(e) => return Err(SoError::Storage(e)),
    };
    if !result.rows.is_empty() {
        return Err(SoError::CycleDetected {
            child: child.to_string(),
            parent: parent.to_string(),
        });
    }
    Ok(())
}
