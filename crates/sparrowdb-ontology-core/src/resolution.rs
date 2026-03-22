use sparrowdb::GraphDb;
use sparrowdb_execution::{QueryResult, Value};

use crate::error::SoError;
use crate::model::AliasKind;
use crate::namespace::{ALIAS_LABEL, CLASS_LABEL, RELATION_LABEL};

// ── Cypher string safety (SPA-218) ────────────────────────────────────────────

/// Escape a user-supplied string for safe interpolation into a Cypher query.
/// Replaces `\` → `\\` and `'` → `\'`.
///
/// TODO SPA-218: Replace with parameterized Cypher when SPA-218 ships in SparrowDB.
pub(crate) fn escape_cypher_string(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\'', "\\'")
}

// ── ResolvedSymbol ────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ResolvedSymbol {
    pub canonical_name: String,
    pub symbol_id: String,
    pub was_alias: bool,
    pub original_name: String,
}

// ── Resolution ────────────────────────────────────────────────────────────────

/// Resolve a name (canonical or alias) to its canonical symbol.
///
/// Two-pass: canonical lookup first, then alias lookup.
/// Returns `SoError::UnknownSymbol` with a list of valid names on miss.
///
/// `kind` is mandatory — the same string may be a class alias AND a relation alias.
///
/// NOTE: SparrowDB has no `ReadTx::query()`. We use `db.execute(cypher)` for all
/// reads. TODO SPA-209: switch to `db.begin_read()?.query()` if that API ships.
pub fn resolve(db: &GraphDb, name: &str, kind: AliasKind) -> Result<ResolvedSymbol, SoError> {
    let canonical_label = match kind {
        AliasKind::Class => CLASS_LABEL,
        AliasKind::Relation => RELATION_LABEL,
    };
    let safe_name = escape_cypher_string(name);

    // Pass 1: canonical match
    // SparrowDB returns InvalidArgument("unknown label") if the label doesn't exist yet → treat as miss.
    let q = format!(
        "MATCH (n:{canonical_label}) WHERE n.name = '{safe_name}' RETURN n.symbol_id, n.name"
    );
    let result = match db.execute(&q) {
        Ok(r) => r,
        Err(sparrowdb_common::Error::InvalidArgument(ref msg)) if msg.contains("unknown label") => {
            sparrowdb_execution::QueryResult::empty(vec![])
        }
        Err(e) => return Err(SoError::Storage(e)),
    };
    if let Some(row) = result.rows.first() {
        let symbol_id = str_from_value(&row[0])?.to_string();
        let canonical_name = str_from_value(&row[1])?.to_string();
        return Ok(ResolvedSymbol {
            canonical_name,
            symbol_id,
            was_alias: false,
            original_name: name.to_string(),
        });
    }

    // Pass 2: alias match
    let kind_str = alias_kind_str(&kind);
    let q = format!(
        "MATCH (a:{ALIAS_LABEL})-[:__SO_ALIAS_OF]->(c:{canonical_label}) \
         WHERE a.name = '{safe_name}' AND a.kind = '{kind_str}' \
         RETURN c.symbol_id, c.name"
    );
    let result = match db.execute(&q) {
        Ok(r) => r,
        Err(sparrowdb_common::Error::InvalidArgument(ref msg)) if msg.contains("unknown label") => {
            sparrowdb_execution::QueryResult::empty(vec![])
        }
        Err(e) => return Err(SoError::Storage(e)),
    };
    if let Some(row) = result.rows.first() {
        let symbol_id = str_from_value(&row[0])?.to_string();
        let canonical_name = str_from_value(&row[1])?.to_string();
        return Ok(ResolvedSymbol {
            canonical_name,
            symbol_id,
            was_alias: true,
            original_name: name.to_string(),
        });
    }

    let valid = list_canonical_names(db, kind)?;
    Err(SoError::UnknownSymbol {
        name: name.to_string(),
        kind: kind_str.to_string(),
        valid,
    })
}

/// Return all canonical names for the given kind.
pub fn list_canonical_names(db: &GraphDb, kind: AliasKind) -> Result<Vec<String>, SoError> {
    let label = match kind {
        AliasKind::Class => CLASS_LABEL,
        AliasKind::Relation => RELATION_LABEL,
    };
    let q = format!("MATCH (n:{label}) RETURN n.name");
    let result = match db.execute(&q) {
        Ok(r) => r,
        Err(sparrowdb_common::Error::InvalidArgument(ref msg)) if msg.contains("unknown label") => {
            sparrowdb_execution::QueryResult::empty(vec![])
        }
        Err(e) => return Err(SoError::Storage(e)),
    };
    let mut names = Vec::new();
    for row in &result.rows {
        if let Some(v) = row.first() {
            if let Value::String(s) = v {
                names.push(s.clone());
            }
        }
    }
    Ok(names)
}

// ── Helpers ───────────────────────────────────────────────────────────────────

pub(crate) fn alias_kind_str(kind: &AliasKind) -> &'static str {
    match kind {
        AliasKind::Class => "class",
        AliasKind::Relation => "relation",
    }
}

/// Extract a &str from a Value::String, or return a Storage error.
pub(crate) fn str_from_value(v: &Value) -> Result<&str, SoError> {
    match v {
        Value::String(s) => Ok(s.as_str()),
        _ => Err(SoError::Storage(sparrowdb_common::Error::InvalidArgument(
            format!("expected string value, got {:?}", v),
        ))),
    }
}

/// Extract an i64 from a Value::Int64, or return a Storage error.
#[allow(dead_code)]
pub(crate) fn i64_from_value(v: &Value) -> Result<i64, SoError> {
    match v {
        Value::Int64(n) => Ok(*n),
        _ => Err(SoError::Storage(sparrowdb_common::Error::InvalidArgument(
            format!("expected int64 value, got {:?}", v),
        ))),
    }
}

/// Extract a bool from a Value::Bool, or false if null/absent.
#[allow(dead_code)]
pub(crate) fn bool_from_value(v: &Value) -> bool {
    match v {
        Value::Bool(b) => *b,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_apostrophe() {
        assert_eq!(escape_cypher_string("O'Reilly"), "O\\'Reilly");
    }

    #[test]
    fn escape_backslash() {
        assert_eq!(escape_cypher_string("a\\b"), "a\\\\b");
    }

    #[test]
    fn escape_both() {
        assert_eq!(escape_cypher_string("a\\'b"), "a\\\\\\'b");
    }
}
