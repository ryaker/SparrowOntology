use sparrowdb::GraphDb;

use crate::error::SoError;
use crate::model::{AliasKind, ResolvedSymbol};
use crate::namespace::*;

/// Escape a user-supplied string for safe Cypher interpolation.
///
/// **TODO SPA-218:** Replace with parameterized Cypher when SPA-218 ships in SparrowDB.
///
/// Handles the O'Reilly Inc apostrophe test and all special characters.
pub fn escape_cypher_string(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\'', "\\'")
}

/// Resolve a symbol name (class, relation, property) to its canonical form.
///
/// Two-pass algorithm:
/// 1. Check if it's a canonical symbol (exists as a __SO_Class, __SO_Relation, etc.)
/// 2. If not found, check if it's an alias and resolve to canonical
///
/// The `kind` parameter is mandatory — the same name can be a class alias AND
/// a relation alias, so we need to know which one to look up.
///
/// Returns `UnknownSymbol` if neither canonical nor alias is found, with a list
/// of valid options for the given kind.
pub fn resolve(
    db: &GraphDb,
    name: &str,
    kind: AliasKind,
) -> Result<ResolvedSymbol, SoError> {
    let label = match kind {
        AliasKind::Class => CLASS_LABEL,
        AliasKind::Relation => RELATION_LABEL,
        AliasKind::Property => PROPERTY_LABEL,
    };

    let escaped_name = escape_cypher_string(name);

    // Pass 1: Check if this is a canonical symbol
    let canonical_query = format!(
        "MATCH (n:{}) WHERE n.name = '{}' RETURN n.name, n.symbol_id, n.description LIMIT 1",
        label, escaped_name
    );

    let result = db
        .begin_read()
        .query(&canonical_query)
        .map_err(|e| SoError::Storage {
            message: e.to_string(),
        })?;

    if !result.rows.is_empty() {
        let row = &result.rows[0];
        return Ok(ResolvedSymbol {
            canonical_name: name.to_string(),
            symbol_id: row.get(1).and_then(|v| v.as_string()).unwrap_or_default().to_string(),
            kind,
            was_alias: false,
            description: row.get(2).and_then(|v| v.as_string()).map(|s| s.to_string()),
        });
    }

    // Pass 2: Check if this is an alias
    let alias_query = format!(
        "MATCH (a:{}) WHERE a.name = '{}' AND a.alias_kind = '{}'
         RETURN a.canonical_name, a.symbol_id, a.description LIMIT 1",
        ALIAS_LABEL, escaped_name, kind
    );

    let alias_result = db
        .begin_read()
        .query(&alias_query)
        .map_err(|e| SoError::Storage {
            message: e.to_string(),
        })?;

    if !alias_result.rows.is_empty() {
        let row = &alias_result.rows[0];
        return Ok(ResolvedSymbol {
            canonical_name: row
                .get(0)
                .and_then(|v| v.as_string())
                .unwrap_or_default()
                .to_string(),
            symbol_id: row.get(1).and_then(|v| v.as_string()).unwrap_or_default().to_string(),
            kind,
            was_alias: true,
            description: row.get(2).and_then(|v| v.as_string()).map(|s| s.to_string()),
        });
    }

    // Not found: return UnknownSymbol with valid options
    let valid_options = list_canonical_names(db, kind)?;
    Err(SoError::unknown_symbol(
        name,
        format!("{:?}", kind),
        valid_options,
    ))
}

/// List all canonical symbol names for a given kind.
///
/// Used to populate the `valid_options` field in `UnknownSymbol` errors.
pub fn list_canonical_names(db: &GraphDb, kind: AliasKind) -> Result<Vec<String>, SoError> {
    let label = match kind {
        AliasKind::Class => CLASS_LABEL,
        AliasKind::Relation => RELATION_LABEL,
        AliasKind::Property => PROPERTY_LABEL,
    };

    let query = format!("MATCH (n:{}) RETURN n.name ORDER BY n.name", label);

    let result = db
        .begin_read()
        .query(&query)
        .map_err(|e| SoError::Storage {
            message: e.to_string(),
        })?;

    let names: Vec<String> = result
        .rows
        .iter()
        .filter_map(|row| {
            row.get(0)
                .and_then(|v| v.as_string())
                .map(|s| s.to_string())
        })
        .collect();

    Ok(names)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_escape_cypher_string_apostrophe() {
        // The O'Reilly test case
        let input = "O'Reilly Inc";
        let escaped = escape_cypher_string(input);
        assert_eq!(escaped, "O\\'Reilly Inc");
    }

    #[test]
    fn test_escape_cypher_string_backslash() {
        let input = "test\\path";
        let escaped = escape_cypher_string(input);
        assert_eq!(escaped, "test\\\\path");
    }

    #[test]
    fn test_escape_cypher_string_both() {
        let input = "test'\\path";
        let escaped = escape_cypher_string(input);
        assert_eq!(escaped, "test\\'\\\\path");
    }
}
