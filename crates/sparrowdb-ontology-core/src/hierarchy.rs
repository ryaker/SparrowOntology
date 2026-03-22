use std::collections::HashSet;

use sparrowdb::GraphDb;
use sparrowdb_execution::Value;

use crate::error::SoError;
use crate::namespace::{CLASS_LABEL, RELATION_LABEL, SUBCLASS_OF_REL, SUBPROPERTY_OF_REL};
use crate::resolution::escape_cypher_string;

// ── BFS helpers (workaround for SPA-224: *N..M var-path broken on __SO_ labels) ──
//
// SparrowDB's variable-length Cypher path queries (*1..N) return empty results
// when src/dst nodes carry __SO_-prefixed labels (engine bug tracked in SPA-224).
// Single-hop queries work correctly. All traversals here use iterative BFS over
// 1-hop queries instead of a single *1..N Cypher expression.
// TODO: simplify back to Cypher *1..N once SPA-224 is fixed upstream.

/// BFS: walk `edge_type` edges forward from `start` (1-hop per iteration).
/// Returns all reachable node names up to `max_depth` hops.
fn bfs_forward(
    db: &GraphDb,
    start: &str,
    node_label: &str,
    edge_type: &str,
    max_depth: usize,
) -> Result<HashSet<String>, SoError> {
    let mut visited: HashSet<String> = HashSet::new();
    let mut frontier = vec![start.to_string()];
    visited.insert(start.to_string());

    for _ in 0..max_depth {
        if frontier.is_empty() {
            break;
        }
        let mut next: Vec<String> = Vec::new();
        for name in &frontier {
            let safe = escape_cypher_string(name);
            let q = format!(
                "MATCH (n:{node_label} {{name: '{safe}'}})-[:{edge_type}]->(p:{node_label}) \
                 RETURN p.name"
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
                if let Some(Value::String(parent)) = row.first() {
                    if visited.insert(parent.clone()) {
                        next.push(parent.clone());
                    }
                }
            }
        }
        frontier = next;
    }
    Ok(visited)
}

// ── Public traversal API ───────────────────────────────────────────────────────

/// Returns `class_name` itself plus all direct and transitive subclasses
/// (nodes that point TO `class_name` via `__SO_SUBCLASS_OF`, up to `depth` hops).
pub fn expand_subclasses(
    db: &GraphDb,
    class_name: &str,
    depth: usize,
) -> Result<Vec<String>, SoError> {
    // Walk REVERSE direction: find all nodes that eventually point to class_name.
    let mut names = vec![class_name.to_string()];
    let mut frontier = vec![class_name.to_string()];
    let mut visited: HashSet<String> = HashSet::new();
    visited.insert(class_name.to_string());

    for _ in 0..depth {
        if frontier.is_empty() {
            break;
        }
        let mut next: Vec<String> = Vec::new();
        for name in &frontier {
            let safe = escape_cypher_string(name);
            let q = format!(
                "MATCH (sub:{CLASS_LABEL})-[:{SUBCLASS_OF_REL}]->(base:{CLASS_LABEL} {{name: '{safe}'}}) \
                 RETURN sub.name"
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
                if let Some(Value::String(s)) = row.first() {
                    if visited.insert(s.clone()) {
                        names.push(s.clone());
                        next.push(s.clone());
                    }
                }
            }
        }
        frontier = next;
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
    let mut names = vec![rel_name.to_string()];
    let mut frontier = vec![rel_name.to_string()];
    let mut visited: HashSet<String> = HashSet::new();
    visited.insert(rel_name.to_string());

    for _ in 0..depth {
        if frontier.is_empty() {
            break;
        }
        let mut next: Vec<String> = Vec::new();
        for name in &frontier {
            let safe = escape_cypher_string(name);
            let q = format!(
                "MATCH (sub:{RELATION_LABEL})-[:{SUBPROPERTY_OF_REL}]->(base:{RELATION_LABEL} {{name: '{safe}'}}) \
                 RETURN sub.name"
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
                if let Some(Value::String(s)) = row.first() {
                    if visited.insert(s.clone()) {
                        names.push(s.clone());
                        next.push(s.clone());
                    }
                }
            }
        }
        frontier = next;
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
    // Walk forward from `parent` via edge_type. If we reach `child`, it's a cycle.
    let reachable = bfs_forward(db, parent, CLASS_LABEL, edge_type, 50)?;
    if reachable.contains(child) {
        return Err(SoError::CycleDetected {
            child: child.to_string(),
            parent: parent.to_string(),
        });
    }
    Ok(())
}
