use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use sparrowdb::GraphDb;
use sparrowdb_storage::node_store::Value as StoreValue;

use crate::error::SoError;
use crate::model::{AliasKind, PropertyType, PropertyValue};
use crate::resolution::resolve;
use crate::validation::ValidationContext;

// ── Template ──────────────────────────────────────────────────────────────────

/// JSON import template (version 1).
///
/// ```json
/// {
///   "version": 1,
///   "class": "Person",
///   "mappings": { "csv_col": "ontology_prop" },
///   "key_field": "id"
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportTemplate {
    pub version: u32,
    /// Ontology class name or alias.
    pub class: String,
    /// Maps input field names → ontology property names.
    pub mappings: HashMap<String, String>,
    /// Optional dedup hint — stored as `_import_key` if present.
    pub key_field: Option<String>,
}

// ── Results ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportError {
    pub row: usize,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportResult {
    pub created: usize,
    pub skipped: usize,
    pub errors: Vec<ImportError>,
}

impl ImportResult {
    fn new() -> Self {
        Self {
            created: 0,
            skipped: 0,
            errors: Vec::new(),
        }
    }

    pub fn error_count(&self) -> usize {
        self.errors.len()
    }
}

// ── Core import logic ─────────────────────────────────────────────────────────

/// Import a slice of string-valued records into the ontology graph.
///
/// Each record is a `HashMap<String, String>` (field_name → raw string value).
/// The template controls which fields are mapped and to which ontology properties.
///
/// Rules:
/// - Fields not in `mappings` are silently ignored.
/// - The mapped property name must be declared on the resolved class.
/// - String values are coerced: `int64` fields are parsed as i64 first; on
///   failure they are stored as Bytes. All others are stored as Bytes.
/// - If `key_field` is set and the record contains that field, the raw value is
///   stored as `_import_key` (a Bytes property) on the node.
/// - If `dry_run` is true, validation runs but no nodes are written.
/// - If `skip_errors` is true, row-level validation failures are collected and
///   the import continues; otherwise the first error aborts and returns `Err`.
///
/// Returns `SoError::UnknownSymbol` immediately if `template.class` cannot be
/// resolved (this is not a skip-able row error).
pub fn import_records(
    db: &GraphDb,
    records: &[HashMap<String, String>],
    template: &ImportTemplate,
    dry_run: bool,
    skip_errors: bool,
) -> Result<ImportResult, SoError> {
    // Resolve the class once — a bad class name is a fatal (non-row) error.
    let resolved = resolve(db, &template.class, AliasKind::Class)?;
    let canonical_label = resolved.canonical_name.clone();

    // Fetch property metadata for the class to know declared types.
    let ctx = ValidationContext::new(db);
    let declared_props = ctx.get_properties_for_class(&resolved.symbol_id)?;

    let mut result = ImportResult::new();

    for (row_idx, record) in records.iter().enumerate() {
        // Build PropertyValue map from the raw record using the template mappings.
        let mut props: HashMap<String, PropertyValue> = HashMap::new();

        for (csv_field, onto_prop) in &template.mappings {
            if let Some(raw) = record.get(csv_field) {
                let pv = coerce_value(raw, onto_prop, &declared_props);
                props.insert(onto_prop.clone(), pv);
            }
            // If the CSV field is absent in this record, just skip it.
        }

        // Validate the entity against the ontology (without _import_key, which is
        // not a declared ontology property — it's a provenance/dedup hint).
        match ctx.validate_entity(&canonical_label, &props, true) {
            Ok(_) => {}
            Err(e) => {
                let msg = e.to_string();
                if skip_errors {
                    result.errors.push(ImportError {
                        row: row_idx + 1,
                        message: msg,
                    });
                    result.skipped += 1;
                    continue;
                } else {
                    return Err(e);
                }
            }
        }

        // Write the node if not dry-running.
        if !dry_run {
            let mut store_props = property_values_to_store(&props);

            // If key_field is set and present in the record, inject _import_key
            // directly into the storage props (bypasses ontology validation —
            // it's a provenance/dedup hint, not a declared property).
            if let Some(ref kf) = template.key_field {
                if let Some(raw) = record.get(kf) {
                    store_props.insert(
                        "_import_key".to_string(),
                        StoreValue::Bytes(raw.as_bytes().to_vec()),
                    );
                }
            }

            let mut tx = db.begin_write()?;
            tx.merge_node(&canonical_label, store_props)?;
            tx.commit()?;
        }

        result.created += 1;
    }

    Ok(result)
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Convert a raw string value to a PropertyValue, using the declared property
/// type to guide coercion.
///
/// Coercion rules:
/// - Int64: try parse as i64; fall back to Bytes.
/// - Float64: try parse as f64; fall back to Bytes.
/// - Bool: "true"/"1" → true; "false"/"0" → false; fall back to Bytes.
/// - String, Date, Variant, unknown: store as String.
fn coerce_value(
    raw: &str,
    prop_name: &str,
    declared: &[crate::model::OntologyProperty],
) -> PropertyValue {
    // Find the declared type for this property.
    let dtype = declared
        .iter()
        .find(|p| p.name == prop_name)
        .map(|p| &p.datatype);

    match dtype {
        Some(PropertyType::Int64) => {
            if let Ok(n) = raw.parse::<i64>() {
                return PropertyValue::Int64(n);
            }
            // Fall back to storing as string so validation can still attempt.
            PropertyValue::String(raw.to_string())
        }
        Some(PropertyType::Float64) => {
            if let Ok(f) = raw.parse::<f64>() {
                return PropertyValue::Float64(f);
            }
            PropertyValue::String(raw.to_string())
        }
        Some(PropertyType::Bool) => match raw.to_lowercase().as_str() {
            "true" | "1" | "yes" => PropertyValue::Bool(true),
            "false" | "0" | "no" => PropertyValue::Bool(false),
            _ => PropertyValue::String(raw.to_string()),
        },
        // String, Date, Variant, or undeclared — store as String.
        _ => PropertyValue::String(raw.to_string()),
    }
}

/// Convert a PropertyValue map to the storage-layer StoreValue map.
fn property_values_to_store(props: &HashMap<String, PropertyValue>) -> HashMap<String, StoreValue> {
    props
        .iter()
        .filter_map(|(k, v)| property_value_to_store(v).map(|sv| (k.clone(), sv)))
        .collect()
}

fn property_value_to_store(v: &PropertyValue) -> Option<StoreValue> {
    match v {
        PropertyValue::String(s) => Some(StoreValue::Bytes(s.as_bytes().to_vec())),
        PropertyValue::Int64(n) => Some(StoreValue::Int64(*n)),
        PropertyValue::Float64(f) => Some(StoreValue::Bytes(f.to_string().as_bytes().to_vec())),
        PropertyValue::Bool(b) => Some(StoreValue::Int64(if *b { 1 } else { 0 })),
        PropertyValue::Null => None,
    }
}
