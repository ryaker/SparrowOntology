use std::collections::HashSet;

use sparrowdb::GraphDb;
use sparrowdb_execution::Value;

use crate::error::SoError;
use crate::namespace::{CLASS_LABEL, RELATION_LABEL, SUBCLASS_OF_REL, SUBPROPERTY_OF_REL};
use crate::resolution::escape_cypher_string;

// ── SparrowDB variable-length path bug workaround ─────────────────────────────
//
// SparrowDB's `*1..N` variable-length path Cypher engine has two bugs:
//   1. It matches zero-length paths (treats `*1..N` as `*0..N`).
//   2. It ignores inline property filters on the endpoint node
//      (e.g. `(p)-[*1..N]->(c {name: 'X'})` matches any `c`, not just `c.name='X'`).
//
// All three functions below use single-hop BFS to avoid these bugs.

/// Returns `class_name` itself plus all direct and transitive subclasses
/// (nodes that point TO `class_name` via `__SO_SUBCLASS_OF`, up to `depth` hops).
pub fn expand_subclasses(
    db: &GraphDb,
    class_name: &str,
    depth: usize,
) -> Result<Vec<String>, SoError> {
    let mut collected: HashSet<String> = HashSet::new();
    collected.insert(class_name.to_string());
    let mut frontier: Vec<String> = vec![class_name.to_string()];

    for _ in 0..depth {
        if frontier.is_empty() {
            break;
        }
        let mut next_frontier: Vec<String> = Vec::new();
        for base_name in &frontier {
            let safe_base = escape_cypher_string(base_name);
            // Find nodes pointing TO base_name via SUBCLASS_OF (direct subclasses).
            let q = format!(
                "MATCH (sub:{CLASS_LABEL})-[:{SUBCLASS_OF_REL}]->(base:{CLASS_LABEL}) \
                 WHERE base.name = '{safe_base}' RETURN sub.name"
            );
            let result = match db.execute(&q) {
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
                if let Some(Value::String(s)) = row.first() {
                    if collected.insert(s.clone()) {
                        next_frontier.push(s.clone());
                    }
                }
            }
        }
        frontier = next_frontier;
    }

    Ok(collected.into_iter().collect())
}

/// Returns `rel_name` itself plus all sub-relations (via `__SO_SUBPROPERTY_OF`,
/// up to `depth` hops).
pub fn expand_subproperties(
    db: &GraphDb,
    rel_name: &str,
    depth: usize,
) -> Result<Vec<String>, SoError> {
    let mut collected: HashSet<String> = HashSet::new();
    collected.insert(rel_name.to_string());
    let mut frontier: Vec<String> = vec![rel_name.to_string()];

    for _ in 0..depth {
        if frontier.is_empty() {
            break;
        }
        let mut next_frontier: Vec<String> = Vec::new();
        for base_name in &frontier {
            let safe_base = escape_cypher_string(base_name);
            let q = format!(
                "MATCH (sub:{RELATION_LABEL})-[:{SUBPROPERTY_OF_REL}]->(base:{RELATION_LABEL}) \
                 WHERE base.name = '{safe_base}' RETURN sub.name"
            );
            let result = match db.execute(&q) {
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
                if let Some(Value::String(s)) = row.first() {
                    if collected.insert(s.clone()) {
                        next_frontier.push(s.clone());
                    }
                }
            }
        }
        frontier = next_frontier;
    }

    Ok(collected.into_iter().collect())
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
    // BFS from `parent` following single-hop `edge_type` edges.
    // Uses WHERE clause for node identity (avoids the inline filter bug).
    let mut visited: HashSet<String> = HashSet::new();
    let mut frontier: Vec<String> = vec![parent.to_string()];

    while let Some(current) = frontier.pop() {
        if !visited.insert(current.clone()) {
            continue; // already explored
        }
        let safe_curr = escape_cypher_string(&current);
        let q = format!(
            "MATCH (p:{CLASS_LABEL})-[:{edge_type}]->(c:{CLASS_LABEL}) \
             WHERE p.name = '{safe_curr}' RETURN c.name"
        );
        let result = match db.execute(&q) {
            Ok(r) => r,
            Err(sparrowdb_common::Error::InvalidArgument(ref msg))
                if msg.contains("unknown label") || msg.contains("unknown relationship type") =>
            {
                continue;
            }
            Err(e) => return Err(SoError::Storage(e)),
        };
        for row in &result.rows {
            if let Some(Value::String(name)) = row.first() {
                if name == child {
                    return Err(SoError::CycleDetected {
                        child: child.to_string(),
                        parent: parent.to_string(),
                    });
                }
                frontier.push(name.clone());
            }
        }
    }
    Ok(())
}
