use std::collections::{HashSet, VecDeque};

use sparrowdb::GraphDb;

use crate::error::SoError;
use crate::namespace::*;
use crate::resolution::escape_cypher_string;

/// Expand a class name to include all its subclasses (transitive closure).
///
/// Uses breadth-first traversal up to the given depth limit to avoid infinite loops
/// in case of accidental cycles.
///
/// Returns all class names that either directly or indirectly inherit from the given class.
pub fn expand_subclasses(
    db: &GraphDb,
    class_name: &str,
    depth: usize,
) -> Result<Vec<String>, SoError> {
    if depth == 0 {
        return Ok(vec![class_name.to_string()]);
    }

    let mut result = vec![class_name.to_string()];
    let mut visited = HashSet::new();
    let mut queue = VecDeque::new();

    visited.insert(class_name.to_string());
    queue.push_back((class_name.to_string(), 0));

    let escaped_class_name = escape_cypher_string(class_name);

    while let Some((current, current_depth)) = queue.pop_front() {
        if current_depth >= depth {
            continue;
        }

        let escaped_current = escape_cypher_string(&current);

        // Query for direct subclasses
        let query = format!(
            "MATCH (parent:{} {{name: '{}'}}) <- [{}] - (child:{})
             RETURN child.name",
            CLASS_LABEL, escaped_current, SUBCLASS_OF, CLASS_LABEL
        );

        let query_result = db
            .begin_read()
            .query(&query)
            .map_err(|e| SoError::Storage {
                message: e.to_string(),
            })?;

        for row in &query_result.rows {
            if let Some(subclass_name) = row.get(0).and_then(|v| v.as_string()) {
                let subclass_str = subclass_name.to_string();
                if !visited.contains(&subclass_str) {
                    visited.insert(subclass_str.clone());
                    result.push(subclass_str.clone());
                    queue.push_back((subclass_str, current_depth + 1));
                }
            }
        }
    }

    Ok(result)
}

/// Expand a property to include all related properties through subproperty relationships.
///
/// Same algorithm as `expand_subclasses`, but for properties and `__SO_SUBPROPERTY_OF` edges.
pub fn expand_subproperties(
    db: &GraphDb,
    property_name: &str,
    depth: usize,
) -> Result<Vec<String>, SoError> {
    if depth == 0 {
        return Ok(vec![property_name.to_string()]);
    }

    let mut result = vec![property_name.to_string()];
    let mut visited = HashSet::new();
    let mut queue = VecDeque::new();

    visited.insert(property_name.to_string());
    queue.push_back((property_name.to_string(), 0));

    let escaped_property_name = escape_cypher_string(property_name);

    while let Some((current, current_depth)) = queue.pop_front() {
        if current_depth >= depth {
            continue;
        }

        let escaped_current = escape_cypher_string(&current);

        // Query for direct subproperties
        let query = format!(
            "MATCH (parent:{} {{name: '{}'}}) <- [{}] - (child:{})
             RETURN child.name",
            PROPERTY_LABEL, escaped_current, SUBPROPERTY_OF, PROPERTY_LABEL
        );

        let query_result = db
            .begin_read()
            .query(&query)
            .map_err(|e| SoError::Storage {
                message: e.to_string(),
            })?;

        for row in &query_result.rows {
            if let Some(subprop_name) = row.get(0).and_then(|v| v.as_string()) {
                let subprop_str = subprop_name.to_string();
                if !visited.contains(&subprop_str) {
                    visited.insert(subprop_str.clone());
                    result.push(subprop_str.clone());
                    queue.push_back((subprop_str, current_depth + 1));
                }
            }
        }
    }

    Ok(result)
}

/// Check that adding an edge from `child` to `parent` with edge type `edge_type`
/// would not create a cycle in the hierarchy.
///
/// Does a depth-first search from `parent` following edges of type `edge_type`.
/// If it reaches `child`, a cycle exists.
///
/// This check must be done before creating `__SO_SUBCLASS_OF` or `__SO_SUBPROPERTY_OF` edges.
pub fn check_no_cycle(
    db: &GraphDb,
    child: &str,
    parent: &str,
    edge_type: &str,
) -> Result<(), SoError> {
    // Quick check: can't be a cycle if parent == child (that's a self-loop, handled separately)
    if child == parent {
        return Err(SoError::CycleDetected {
            child: child.to_string(),
            parent: parent.to_string(),
            edge_type: edge_type.to_string(),
        });
    }

    let mut visited = HashSet::new();
    let mut stack = vec![parent.to_string()];

    let escaped_child = escape_cypher_string(child);

    while let Some(current) = stack.pop() {
        if visited.contains(&current) {
            continue;
        }
        visited.insert(current.clone());

        // If we reach the child, we have a cycle
        if current == child {
            return Err(SoError::CycleDetected {
                child: child.to_string(),
                parent: parent.to_string(),
                edge_type: edge_type.to_string(),
            });
        }

        let escaped_current = escape_cypher_string(&current);

        // Find all parents of the current node (follow the edge_type backwards)
        let query = if edge_type == SUBCLASS_OF {
            format!(
                "MATCH (current:{} {{name: '{}'}}) <- [{}] - (ancestor:{})
                 RETURN ancestor.name",
                CLASS_LABEL, escaped_current, edge_type, CLASS_LABEL
            )
        } else {
            // For SUBPROPERTY_OF or other types
            format!(
                "MATCH (current:{} {{name: '{}'}}) <- [{}] - (ancestor:{})
                 RETURN ancestor.name",
                PROPERTY_LABEL, escaped_current, edge_type, PROPERTY_LABEL
            )
        };

        let result = db
            .begin_read()
            .query(&query)
            .map_err(|e| SoError::Storage {
                message: e.to_string(),
            })?;

        for row in &result.rows {
            if let Some(ancestor_name) = row.get(0).and_then(|v| v.as_string()) {
                let ancestor_str = ancestor_name.to_string();
                if !visited.contains(&ancestor_str) {
                    stack.push(ancestor_str);
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_escape_used_in_hierarchy_expansion() {
        // The escape function should be available and work
        let test_name = "Class'With'Quotes";
        let escaped = escape_cypher_string(test_name);
        assert!(escaped.contains("\\'"));
    }
}
