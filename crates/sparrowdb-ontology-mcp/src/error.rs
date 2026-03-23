use serde_json::{json, Value};
use sparrowdb_ontology_core::SoError;

/// Convert a `SoError` into an MCP error data object.
///
/// The returned object carries:
/// - `error_kind`: the variant name (e.g. "UnknownSymbol")
/// - `detail`: human-readable message
/// - `valid_options`: list of valid alternatives (where applicable)
/// - `suggestion`: actionable hint (present for UnknownSymbol, DomainViolation, RangeViolation)
pub fn so_error_to_mcp(e: &SoError) -> Value {
    match e {
        SoError::UnknownSymbol { name, kind, valid, closest_match, suggestion: fuzzy_suggestion } => {
            let suggestion = if let Some(s) = fuzzy_suggestion {
                s.clone()
            } else if valid.is_empty() {
                format!("No {kind} symbols have been defined yet. Use define_class or define_relation first.")
            } else {
                format!(
                    "'{name}' is not a known {kind}. Did you mean one of: {}?",
                    valid.join(", ")
                )
            };
            let mut obj = json!({
                "error_kind": "UnknownSymbol",
                "detail": e.to_string(),
                "valid_options": valid,
                "suggestion": suggestion,
            });
            if let Some(m) = closest_match {
                obj["closest_match"] = json!(m);
            }
            obj
        }

        SoError::AliasConflict { alias, existing, kind } => {
            json!({
                "error_kind": "AliasConflict",
                "detail": e.to_string(),
                "alias": alias,
                "existing_target": existing,
                "kind": kind,
            })
        }

        SoError::CycleDetected { child, parent } => {
            json!({
                "error_kind": "CycleDetected",
                "detail": e.to_string(),
                "child": child,
                "parent": parent,
            })
        }

        SoError::DomainViolation { relation, expected, actual } => {
            json!({
                "error_kind": "DomainViolation",
                "detail": e.to_string(),
                "relation": relation,
                "expected_domain": expected,
                "actual_source": actual,
                "suggestion": format!(
                    "Relation '{relation}' requires the source entity to be of class '{expected}', \
                     but got '{actual}'. Call explain_symbol('{relation}') to see full domain/range constraints."
                ),
            })
        }

        SoError::RangeViolation { relation, expected, actual } => {
            json!({
                "error_kind": "RangeViolation",
                "detail": e.to_string(),
                "relation": relation,
                "expected_range": expected,
                "actual_target": actual,
                "suggestion": format!(
                    "Relation '{relation}' requires the target entity to be of class '{expected}', \
                     but got '{actual}'. Call explain_symbol('{relation}') to see full domain/range constraints."
                ),
            })
        }

        SoError::RequiredPropertyMissing { class, property } => {
            json!({
                "error_kind": "RequiredPropertyMissing",
                "detail": e.to_string(),
                "class": class,
                "property": property,
            })
        }

        SoError::TypeMismatch { class, property, expected, actual } => {
            json!({
                "error_kind": "TypeMismatch",
                "detail": e.to_string(),
                "class": class,
                "property": property,
                "expected_type": expected,
                "actual_type": actual,
            })
        }

        SoError::ReservedNamespace(name) => {
            json!({
                "error_kind": "ReservedNamespace",
                "detail": e.to_string(),
                "name": name,
            })
        }

        SoError::ReservedProperty(prop) => {
            json!({
                "error_kind": "ReservedProperty",
                "detail": e.to_string(),
                "property": prop,
            })
        }

        SoError::DuplicateProperty { class, property } => {
            json!({
                "error_kind": "DuplicateProperty",
                "detail": e.to_string(),
                "class": class,
                "property": property,
                "suggestion": format!(
                    "Property '{property}' is already declared on '{class}'. \
                     Call explain_symbol('{class}') to see all existing properties."
                ),
            })
        }

        SoError::UnseedeedClass { class_name } => {
            json!({
                "error_kind": "UnseedeedClass",
                "detail": e.to_string(),
                "class_name": class_name,
                "suggestion": format!(
                    "Class '{class_name}' has no declared properties. \
                     Call add_property(owner='{class_name}', name='...') for each property \
                     before writing entities. Call start_here to see all unseeded_classes."
                ),
            })
        }

        SoError::AlreadyInitialized => {
            json!({
                "error_kind": "AlreadyInitialized",
                "detail": e.to_string(),
            })
        }

        SoError::Storage(inner) => {
            json!({
                "error_kind": "Storage",
                "detail": inner.to_string(),
            })
        }
    }
}

/// Build a JSON-RPC error object for embedding in a `JsonRpcResponse.error`.
pub fn mcp_error(code: i64, message: &str, data: Value) -> Value {
    json!({
        "code": code,
        "message": message,
        "data": data,
    })
}

/// Build an MCP error JSON-RPC object from a `SoError`, surfacing the fuzzy
/// suggestion prominently in the `message` field when available.
pub fn so_error_to_mcp_error(code: i64, context: &str, e: &SoError) -> Value {
    let data = so_error_to_mcp(e);
    let message = match e {
        SoError::UnknownSymbol { name: _, kind: _, valid: _, closest_match: Some(_), suggestion: Some(s) } => {
            format!("{context}: {s}")
        }
        _ => context.to_string(),
    };
    mcp_error(code, &message, data)
}
