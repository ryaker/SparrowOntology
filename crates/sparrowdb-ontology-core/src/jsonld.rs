//! JSON-LD export for the SparrowOntology schema.
//!
//! `export_json_ld` reads the full ontology from a `GraphDb` and returns a
//! JSON-LD 1.1 document conforming to the OWL/RDFS/SKOS vocabulary used by
//! SparrowOntology.
//!
//! The output shape:
//! ```json
//! {
//!   "@context": { "owl": "...", "rdfs": "...", ... },
//!   "@graph": [ { "@id": "...", "@type": "owl:Class", ... }, ... ]
//! }
//! ```
//!
//! Rules:
//! - `@id` is `class.iri` (if set and non-empty) else `"so:" + symbol_id`.
//! - Optional fields (`rdfs:comment`, `skos:altLabel`, etc.) are omitted when
//!   empty/absent.
//! - Relation `rdfs:domain` / `rdfs:range` resolve to the domain/range class
//!   `@id` using a name→IRI map built from all classes.

use std::collections::HashMap;

use serde_json::{json, Map, Value as JsVal};
use sparrowdb::GraphDb;

use crate::error::SoError;
use crate::snapshot::export_schema;

// ── IRI helpers ───────────────────────────────────────────────────────────────

/// Return the `@id` string for a class or relation.
/// Uses the stored IRI if present and non-empty, otherwise falls back to
/// `"so:" + symbol_id`.
fn node_id(iri: &Option<String>, symbol_id: &str) -> String {
    match iri {
        Some(s) if !s.is_empty() => s.clone(),
        _ => format!("so:{symbol_id}"),
    }
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Export the full ontology schema as a JSON-LD document.
///
/// Reads all `__SO_Class`, `__SO_Relation`, `__SO_Alias`, and subclass edges
/// from `db` and serialises them into a JSON-LD 1.1 graph.
///
/// # Errors
/// Returns `SoError` if any database query fails.
pub fn export_json_ld(db: &GraphDb) -> Result<JsVal, SoError> {
    // Reuse the snapshot exporter — it handles the two-scan+zip workaround for
    // the SparrowDB ≤0.1.6 multi-column label-scan bug and populates all fields
    // including IRI, aliases, and subclass edges.
    let snap = export_schema(db)?;

    // ── Build helper maps ─────────────────────────────────────────────────────

    // class name → @id (for resolving domain/range of relations)
    let class_id_by_name: HashMap<&str, String> = snap
        .classes
        .iter()
        .map(|c| (c.name.as_str(), node_id(&c.iri, &c.symbol_id)))
        .collect();

    // class symbol_id → @id (for resolving subClassOf parent)
    let class_id_by_sid: HashMap<&str, String> = snap
        .classes
        .iter()
        .map(|c| (c.symbol_id.as_str(), node_id(&c.iri, &c.symbol_id)))
        .collect();

    // class symbol_id → parent symbol_id (first parent; classes are trees here)
    let parent_sid_by_child: HashMap<&str, &str> = snap
        .subclass_edges
        .iter()
        .map(|(child, parent)| (child.as_str(), parent.as_str()))
        .collect();

    // class/relation symbol_id → Vec<alias names>
    let mut aliases_by_sid: HashMap<&str, Vec<&str>> = HashMap::new();
    for alias in &snap.aliases {
        aliases_by_sid
            .entry(alias.target_symbol_id.as_str())
            .or_default()
            .push(alias.name.as_str());
    }

    // ── Property maps (per class symbol_id) ──────────────────────────────────

    // symbol_id → Vec<required property names>
    let mut required_props_by_class: HashMap<&str, Vec<&str>> = HashMap::new();
    // symbol_id → Vec<all property names>
    let mut allowed_props_by_class: HashMap<&str, Vec<&str>> = HashMap::new();
    // symbol_id → { prop_name: [allowed_values] }
    let mut prop_allowed_values_by_class: HashMap<&str, HashMap<&str, Vec<&str>>> = HashMap::new();

    for prop in &snap.properties {
        let sid = prop.owner_symbol_id.as_str();

        allowed_props_by_class
            .entry(sid)
            .or_default()
            .push(prop.name.as_str());

        if prop.required {
            required_props_by_class
                .entry(sid)
                .or_default()
                .push(prop.name.as_str());
        }

        if let Some(ref vals) = prop.allowed_values {
            if !vals.is_empty() {
                prop_allowed_values_by_class
                    .entry(sid)
                    .or_default()
                    .insert(prop.name.as_str(), vals.iter().map(|s| s.as_str()).collect());
            }
        }
    }

    // ── Build @graph ──────────────────────────────────────────────────────────

    let mut graph: Vec<JsVal> = Vec::new();

    // Classes
    for class in &snap.classes {
        let id = node_id(&class.iri, &class.symbol_id);
        let sid = class.symbol_id.as_str();

        let mut obj = Map::new();
        obj.insert("@id".into(), json!(id));
        obj.insert("@type".into(), json!("owl:Class"));
        obj.insert("rdfs:label".into(), json!(class.name));

        if let Some(ref desc) = class.description {
            if !desc.is_empty() {
                obj.insert("rdfs:comment".into(), json!(desc));
            }
        }

        // skos:altLabel — aliases
        if let Some(alias_names) = aliases_by_sid.get(sid) {
            if !alias_names.is_empty() {
                obj.insert(
                    "skos:altLabel".into(),
                    JsVal::Array(alias_names.iter().map(|a| json!(*a)).collect()),
                );
            }
        }

        // rdfs:subClassOf — first parent in subclass edges
        if let Some(parent_sid) = parent_sid_by_child.get(sid) {
            if let Some(parent_id) = class_id_by_sid.get(parent_sid) {
                obj.insert(
                    "rdfs:subClassOf".into(),
                    json!({ "@id": parent_id }),
                );
            }
        }

        // so:requiredProperties
        if let Some(req_props) = required_props_by_class.get(sid) {
            if !req_props.is_empty() {
                obj.insert(
                    "so:requiredProperties".into(),
                    JsVal::Array(req_props.iter().map(|p| json!(*p)).collect()),
                );
            }
        }

        // so:allowedProperties
        if let Some(all_props) = allowed_props_by_class.get(sid) {
            if !all_props.is_empty() {
                obj.insert(
                    "so:allowedProperties".into(),
                    JsVal::Array(all_props.iter().map(|p| json!(*p)).collect()),
                );
            }
        }

        // so:allowedValues — { prop_name: ["v1", "v2", ...] }
        if let Some(av_map) = prop_allowed_values_by_class.get(sid) {
            if !av_map.is_empty() {
                let mut av_obj = Map::new();
                for (prop_name, vals) in av_map {
                    av_obj.insert(
                        (*prop_name).to_string(),
                        JsVal::Array(vals.iter().map(|v| json!(*v)).collect()),
                    );
                }
                obj.insert("so:allowedValues".into(), JsVal::Object(av_obj));
            }
        }

        obj.insert("so:symbolId".into(), json!(class.symbol_id));

        graph.push(JsVal::Object(obj));
    }

    // Relations
    for rel in &snap.relations {
        let id = node_id(&rel.iri, &rel.symbol_id);
        let sid = rel.symbol_id.as_str();

        let mut obj = Map::new();
        obj.insert("@id".into(), json!(id));
        obj.insert("@type".into(), json!("owl:ObjectProperty"));
        obj.insert("rdfs:label".into(), json!(rel.name));

        if let Some(ref desc) = rel.description {
            if !desc.is_empty() {
                obj.insert("rdfs:comment".into(), json!(desc));
            }
        }

        // skos:altLabel — aliases
        if let Some(alias_names) = aliases_by_sid.get(sid) {
            if !alias_names.is_empty() {
                obj.insert(
                    "skos:altLabel".into(),
                    JsVal::Array(alias_names.iter().map(|a| json!(*a)).collect()),
                );
            }
        }

        // rdfs:domain — resolve class name → @id
        if !rel.domain.is_empty() {
            if let Some(domain_id) = class_id_by_name.get(rel.domain.as_str()) {
                obj.insert("rdfs:domain".into(), json!({ "@id": domain_id }));
            } else {
                // Fallback: store the name as-is (class might not be in this DB)
                obj.insert("rdfs:domain".into(), json!({ "@id": rel.domain }));
            }
        }

        // rdfs:range — resolve class name → @id
        if !rel.range.is_empty() {
            if let Some(range_id) = class_id_by_name.get(rel.range.as_str()) {
                obj.insert("rdfs:range".into(), json!({ "@id": range_id }));
            } else {
                obj.insert("rdfs:range".into(), json!({ "@id": rel.range }));
            }
        }

        // so:required — only if the relation itself has required flag
        // Note: OntologyRelation does not have a `required` field; this maps to
        // the relation property's `required` boolean if defined as a property.
        // Per issue spec: only emit if true. Since OntologyRelation has no
        // `required` field, we skip this. (Issue example shows it on relations
        // that have a required property declared — omit for now as model
        // doesn't carry it at the relation level.)

        obj.insert("so:symbolId".into(), json!(rel.symbol_id));

        graph.push(JsVal::Object(obj));
    }

    // ── Assemble document ─────────────────────────────────────────────────────

    let context = json!({
        "owl":  "http://www.w3.org/2002/07/owl#",
        "rdfs": "http://www.w3.org/2000/01/rdf-schema#",
        "xsd":  "http://www.w3.org/2001/XMLSchema#",
        "skos": "http://www.w3.org/2004/02/skos/core#",
        "so":   "http://sparrowontology.io/schema#"
    });

    Ok(json!({
        "@context": context,
        "@graph": graph
    }))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_id_uses_iri_when_set() {
        let iri = Some("https://example.org/Foo".to_string());
        assert_eq!(node_id(&iri, "abc-123"), "https://example.org/Foo");
    }

    #[test]
    fn node_id_falls_back_to_symbol_id() {
        assert_eq!(node_id(&None, "abc-123"), "so:abc-123");
    }

    #[test]
    fn node_id_empty_iri_falls_back() {
        let iri = Some(String::new());
        assert_eq!(node_id(&iri, "abc-123"), "so:abc-123");
    }
}
