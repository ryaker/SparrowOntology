use std::collections::HashMap;
use std::time::Instant;

use serde_json::{json, Value};
use sparrowdb::GraphDb;
use sparrowdb_execution::Value as ExecValue;
use sparrowdb_ontology_core::hierarchy::{expand_subclasses, expand_subproperties};
use sparrowdb_ontology_core::model::{AliasKind, PropertyValue};
use sparrowdb_ontology_core::namespace::{
    ALIAS_LABEL, ALIAS_OF_REL, CLASS_LABEL, DOMAIN_REL, HAS_PROPERTY_REL, PROPERTY_LABEL,
    RANGE_REL, RELATION_LABEL, SUBCLASS_OF_REL, SUBPROPERTY_OF_REL,
};
use sparrowdb_common::NodeId;
use sparrowdb_ontology_core::{resolve, ValidationContext};
use sparrowdb_storage::node_store::Value as StoreValue;

use crate::error::{mcp_error, so_error_to_mcp, so_error_to_mcp_error};

// ── Cypher string escaping ────────────────────────────────────────────────────

fn escape_cypher_string(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\'', "\\'")
}

// ── Execute-or-empty helper ───────────────────────────────────────────────────

fn execute_or_empty(db: &GraphDb, q: &str) -> Result<sparrowdb_execution::QueryResult, Value> {
    match db.execute(q) {
        Ok(r) => Ok(r),
        Err(sparrowdb_common::Error::InvalidArgument(ref msg))
            if msg.contains("unknown label") || msg.contains("unknown relationship type") =>
        {
            Ok(sparrowdb_execution::QueryResult::empty(vec![]))
        }
        Err(e) => Err(mcp_error(
            -32603,
            "Database error",
            json!({"detail": e.to_string()}),
        )),
    }
}

// ── Row value helpers ─────────────────────────────────────────────────────────

fn str_val(row: &[ExecValue], idx: usize) -> String {
    row.get(idx)
        .and_then(|v| if let ExecValue::String(s) = v { Some(s.clone()) } else { None })
        .unwrap_or_default()
}

fn int_val(row: &[ExecValue], idx: usize) -> i64 {
    row.get(idx)
        .and_then(|v| if let ExecValue::Int64(n) = v { Some(*n) } else { None })
        .unwrap_or(0)
}

// ── JSON → PropertyValue ──────────────────────────────────────────────────────

fn json_to_property_value(v: &Value) -> PropertyValue {
    match v {
        Value::String(s) => PropertyValue::String(s.clone()),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                PropertyValue::Int64(i)
            } else if let Some(f) = n.as_f64() {
                PropertyValue::Float64(f)
            } else {
                PropertyValue::Null
            }
        }
        Value::Bool(b) => PropertyValue::Bool(*b),
        Value::Null => PropertyValue::Null,
        _ => PropertyValue::Null,
    }
}

// ── PropertyValue → StoreValue ────────────────────────────────────────────────

fn property_value_to_store(v: &PropertyValue) -> Option<StoreValue> {
    match v {
        PropertyValue::String(s) => Some(StoreValue::Bytes(s.as_bytes().to_vec())),
        PropertyValue::Int64(n) => Some(StoreValue::Int64(*n)),
        PropertyValue::Float64(f) => {
            // Store floats as bytes (string representation) since StoreValue may not have Float64
            Some(StoreValue::Bytes(f.to_string().as_bytes().to_vec()))
        }
        PropertyValue::Bool(b) => Some(StoreValue::Int64(if *b { 1 } else { 0 })),
        PropertyValue::Null => None,
    }
}

fn props_to_store(props: &HashMap<String, PropertyValue>) -> HashMap<String, StoreValue> {
    props
        .iter()
        .filter_map(|(k, v)| property_value_to_store(v).map(|sv| (k.clone(), sv)))
        .collect()
}


// ── Node label lookup by integer ID ──────────────────────────────────────────

fn get_node_label(db: &GraphDb, node_id: i64) -> Result<String, Value> {
    // labels(n) returns Value::List([Value::String(label)]) — subscript [0] not supported
    // by the query engine, so we RETURN labels(n) and extract the first element in Rust.
    let q = format!("MATCH (n) WHERE id(n) = {node_id} RETURN labels(n)");
    let result = execute_or_empty(db, &q)?;
    result
        .rows
        .first()
        .and_then(|row| row.first())
        .and_then(|v| match v {
            // The engine returns Value::List([Value::String(label)])
            ExecValue::List(list) => list.first().and_then(|item| {
                if let ExecValue::String(s) = item {
                    Some(s.clone())
                } else {
                    None
                }
            }),
            // Fallback: sometimes it may be a plain string (shouldn't happen but be safe)
            ExecValue::String(s) => Some(s.clone()),
            _ => None,
        })
        .ok_or_else(|| mcp_error(-32602, "Node not found", json!({"node_id": node_id})))
}

// ── Dispatch ──────────────────────────────────────────────────────────────────

pub fn dispatch(db: &GraphDb, name: &str, params: Option<Value>) -> Result<Value, Value> {
    match name {
        "create_entity" => create_entity(db, params),
        "create_relationship" => create_relationship(db, params),
        "update_entity" => update_entity(db, params),
        "find_entities" => find_entities(db, params),
        "explain_symbol" => explain_symbol(db, params),
        "validate" => validate(db, params),
        _ => Err(mcp_error(-32601, "Method not found", json!({"tool": name}))),
    }
}

// ── create_entity ─────────────────────────────────────────────────────────────

pub fn create_entity(db: &GraphDb, params: Option<Value>) -> Result<Value, Value> {
    let args = params.unwrap_or(json!({}));

    // Accept both "class_name" (schema-advertised) and "label" (legacy) for backward compat.
    let label = args["class_name"]
        .as_str()
        .or_else(|| args["label"].as_str())
        .ok_or_else(|| mcp_error(-32602, "Missing required param: class_name", json!({})))?;
    let preserve_source_terms = args["preserve_source_terms"].as_bool().unwrap_or(false);

    // Step 1: resolve label
    let resolved = resolve(db, label, AliasKind::Class)
        .map_err(|e| so_error_to_mcp_error(-32602, "Label resolution failed", &e))?;

    // Step 2: build property map from JSON
    let mut props: HashMap<String, PropertyValue> = HashMap::new();
    if let Some(obj) = args["properties"].as_object() {
        for (k, v) in obj {
            props.insert(k.clone(), json_to_property_value(v));
        }
    }

    // Step 3: validate entity
    ValidationContext::new(db)
        .validate_entity(&resolved.canonical_name, &props, true)
        .map_err(|e| mcp_error(-32602, "Validation failed", so_error_to_mcp(&e)))?;

    // Step 4: optionally inject __so_source_label
    if preserve_source_terms && resolved.was_alias {
        props.insert(
            "__so_source_label".to_string(),
            PropertyValue::String(resolved.original_name.clone()),
        );
    }

    // Step 5: write node via WriteTx::merge_node (CREATE ... RETURN not supported by engine)
    let canonical_label = &resolved.canonical_name;
    let store_props = props_to_store(&props);
    let node_id = {
        let mut tx = db.begin_write().map_err(|e| {
            mcp_error(-32603, "Failed to begin write", json!({"detail": e.to_string()}))
        })?;
        let nid = tx
            .merge_node(canonical_label, store_props)
            .map_err(|e| mcp_error(-32603, "Failed to create entity", json!({"detail": e.to_string()})))?;
        tx.commit().map_err(|e| {
            mcp_error(-32603, "Failed to commit entity", json!({"detail": e.to_string()}))
        })?;
        nid.0 as i64
    };

    // Step 6: return
    let source_label_val = if resolved.was_alias {
        json!(resolved.original_name)
    } else {
        json!(null)
    };

    Ok(json!({
        "content": [{
            "type": "text",
            "text": serde_json::to_string(&json!({
                "created": true,
                "node_id": node_id.to_string(),
                "canonical_label": resolved.canonical_name,
                "source_label": source_label_val,
            })).unwrap_or_default()
        }]
    }))
}

// ── create_relationship ───────────────────────────────────────────────────────

pub fn create_relationship(db: &GraphDb, params: Option<Value>) -> Result<Value, Value> {
    let args = params.unwrap_or(json!({}));

    let from_id_str = args["from_id"]
        .as_str()
        .ok_or_else(|| mcp_error(-32602, "Missing required param: from_id", json!({})))?;
    // Accept both "relation_name" (schema-advertised) and "rel_type" (legacy) for backward compat.
    let rel_type = args["relation_name"]
        .as_str()
        .or_else(|| args["rel_type"].as_str())
        .ok_or_else(|| mcp_error(-32602, "Missing required param: relation_name", json!({})))?;
    let to_id_str = args["to_id"]
        .as_str()
        .ok_or_else(|| mcp_error(-32602, "Missing required param: to_id", json!({})))?;

    let from_id: i64 = from_id_str.parse().map_err(|_| {
        mcp_error(-32602, "from_id must be a numeric string", json!({"from_id": from_id_str}))
    })?;
    let to_id: i64 = to_id_str.parse().map_err(|_| {
        mcp_error(-32602, "to_id must be a numeric string", json!({"to_id": to_id_str}))
    })?;

    // Step 1 & 2: look up source and target node labels
    let source_label = get_node_label(db, from_id)?;
    let target_label = get_node_label(db, to_id)?;

    // Step 3: resolve labels
    let source_resolved = resolve(db, &source_label, AliasKind::Class)
        .map_err(|e| so_error_to_mcp_error(-32602, "Source label resolution failed", &e))?;
    let target_resolved = resolve(db, &target_label, AliasKind::Class)
        .map_err(|e| so_error_to_mcp_error(-32602, "Target label resolution failed", &e))?;

    // Step 4: validate relationship
    let rel_resolved = ValidationContext::new(db)
        .validate_relationship(rel_type, &source_resolved.canonical_name, &target_resolved.canonical_name)
        .map_err(|e| mcp_error(-32602, "Relationship validation failed", so_error_to_mcp(&e)))?;

    // Step 5: build edge properties
    let mut edge_props: HashMap<String, PropertyValue> = HashMap::new();
    if let Some(obj) = args["properties"].as_object() {
        for (k, v) in obj {
            edge_props.insert(k.clone(), json_to_property_value(v));
        }
    }

    // Write edge using WriteTx::create_edge (Cypher MATCH+CREATE can't filter by id())
    let src_node_id = NodeId(from_id as u64);
    let dst_node_id = NodeId(to_id as u64);
    let store_edge_props: HashMap<String, StoreValue> = props_to_store(&edge_props);
    // convert StoreValue → WriteTx-compatible Value (sparrowdb_storage)
    // create_edge takes HashMap<String, sparrowdb_storage::node_store::Value>
    // which is the same as StoreValue
    {
        let mut tx = db.begin_write().map_err(|e| {
            mcp_error(-32603, "Database error", json!({"detail": e.to_string()}))
        })?;
        tx.create_edge(src_node_id, dst_node_id, &rel_resolved.canonical_name, store_edge_props)
            .map_err(|e| {
                mcp_error(-32603, "Failed to create relationship", json!({"detail": e.to_string()}))
            })?;
        tx.commit().map_err(|e| {
            mcp_error(-32603, "Failed to commit relationship", json!({"detail": e.to_string()}))
        })?;
    }

    // Step 6: return
    Ok(json!({
        "content": [{
            "type": "text",
            "text": serde_json::to_string(&json!({
                "created": true,
                "from_id": from_id_str,
                "rel_type": rel_resolved.canonical_name,
                "to_id": to_id_str,
            })).unwrap_or_default()
        }]
    }))
}

// ── update_entity ─────────────────────────────────────────────────────────────

pub fn update_entity(db: &GraphDb, params: Option<Value>) -> Result<Value, Value> {
    let args = params.unwrap_or(json!({}));

    let node_id_str = args["node_id"]
        .as_str()
        .ok_or_else(|| mcp_error(-32602, "Missing required param: node_id", json!({})))?;
    let node_id: i64 = node_id_str.parse().map_err(|_| {
        mcp_error(-32602, "node_id must be a numeric string", json!({"node_id": node_id_str}))
    })?;

    // Step 1: look up label
    let label = get_node_label(db, node_id)?;

    // Step 2: resolve label
    let resolved = resolve(db, &label, AliasKind::Class)
        .map_err(|e| so_error_to_mcp_error(-32602, "Label resolution failed", &e))?;

    // Step 3: build property map
    let mut props: HashMap<String, PropertyValue> = HashMap::new();
    if let Some(obj) = args["properties"].as_object() {
        for (k, v) in obj {
            props.insert(k.clone(), json_to_property_value(v));
        }
    }

    if props.is_empty() {
        return Ok(json!({
            "content": [{
                "type": "text",
                "text": serde_json::to_string(&json!({
                    "updated": true,
                    "node_id": node_id_str,
                    "properties_set": [],
                })).unwrap_or_default()
            }]
        }));
    }

    // Step 4: validate (is_create = false — skips required check)
    ValidationContext::new(db)
        .validate_entity(&resolved.canonical_name, &props, false)
        .map_err(|e| mcp_error(-32602, "Validation failed", so_error_to_mcp(&e)))?;

    // Step 5: set each property using WriteTx::set_property (Cypher SET doesn't support id() WHERE)
    let target_node_id = NodeId(node_id as u64);
    let mut properties_set: Vec<String> = Vec::new();
    {
        let mut tx = db.begin_write().map_err(|e| {
            mcp_error(-32603, "Failed to begin write", json!({"detail": e.to_string()}))
        })?;
        for (key, value) in &props {
            if let Some(sv) = property_value_to_store(value) {
                tx.set_property(target_node_id, key, sv)
                    .map_err(|e| {
                        mcp_error(
                            -32603,
                            "Failed to set property",
                            json!({"key": key, "detail": e.to_string()}),
                        )
                    })?;
                properties_set.push(key.clone());
            }
        }
        tx.commit().map_err(|e| {
            mcp_error(-32603, "Failed to commit property update", json!({"detail": e.to_string()}))
        })?;
    }

    // Step 6: return
    Ok(json!({
        "content": [{
            "type": "text",
            "text": serde_json::to_string(&json!({
                "updated": true,
                "node_id": node_id_str,
                "properties_set": properties_set,
            })).unwrap_or_default()
        }]
    }))
}

// ── find_entities ─────────────────────────────────────────────────────────────

pub fn find_entities(db: &GraphDb, params: Option<Value>) -> Result<Value, Value> {
    let args = params.unwrap_or(json!({}));

    // Accept both "class_name" (schema-advertised) and "label" (legacy) for backward compat.
    let label = args["class_name"]
        .as_str()
        .or_else(|| args["label"].as_str())
        .ok_or_else(|| mcp_error(-32602, "Missing required param: class_name", json!({})))?;
    let include_subclasses = args["include_subclasses"].as_bool().unwrap_or(false);
    let limit = args["limit"].as_u64().unwrap_or(20) as usize;
    let offset = args["offset"].as_u64().unwrap_or(0) as usize;

    // Step 1: resolve label
    let resolved = resolve(db, label, AliasKind::Class)
        .map_err(|e| so_error_to_mcp_error(-32602, "Label resolution failed", &e))?;
    let canonical = resolved.canonical_name.clone();

    // Step 2: optionally expand subclasses
    let class_names: Vec<String> = if include_subclasses {
        expand_subclasses(db, &canonical, 20)
            .map_err(|e| mcp_error(-32603, "Subclass expansion failed", so_error_to_mcp(&e)))?
    } else {
        vec![canonical.clone()]
    };

    // Step 3: build WHERE clause from "filters" parameter
    // Note: backtick quoting not supported by engine — use plain property names.
    // For multi-label subclass expansion, run per-label queries and merge.
    let mut where_clauses: Vec<String> = Vec::new();

    // Add property equality filters from "filters" object
    if let Some(obj) = args["filters"].as_object() {
        for (k, v) in obj {
            let safe_key = escape_cypher_string(k);
            match v {
                Value::String(s) => {
                    let safe_val = escape_cypher_string(s);
                    where_clauses.push(format!("n.{safe_key} = '{safe_val}'"));
                }
                Value::Number(n) => {
                    if let Some(i) = n.as_i64() {
                        where_clauses.push(format!("n.{safe_key} = {i}"));
                    } else if let Some(f) = n.as_f64() {
                        where_clauses.push(format!("n.{safe_key} = {f}"));
                    }
                }
                Value::Bool(b) => {
                    where_clauses.push(format!("n.{safe_key} = {b}"));
                }
                _ => {}
            }
        }
    }

    let where_str = if where_clauses.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", where_clauses.join(" AND "))
    };

    // Step 4: build query — run once per label and merge (avoids labels(n)[0] subscript)
    // The engine doesn't support subscript indexing on labels(n).
    // labels(n) returns List([String(label)]) — use RETURN labels(n) and extract in Rust.
    let mut all_rows: Vec<(i64, String, Value)> = Vec::new();
    let labels_to_query: Vec<String> = class_names.clone();

    for lbl in &labels_to_query {
        let safe_lbl = escape_cypher_string(lbl);
        let q = format!(
            "MATCH (n:{safe_lbl}){where_str} RETURN id(n), labels(n), n SKIP {offset} LIMIT {limit}"
        );
        let result = execute_or_empty(db, &q)?;
        for row in &result.rows {
            let node_id = row
                .first()
                .and_then(|v| if let ExecValue::Int64(n) = v { Some(*n) } else { None })
                .unwrap_or(0);
            // labels(n) returns List([String(label)])
            let row_label = row
                .get(1)
                .and_then(|v| match v {
                    ExecValue::List(list) => list.first().and_then(|item| {
                        if let ExecValue::String(s) = item { Some(s.clone()) } else { None }
                    }),
                    ExecValue::String(s) => Some(s.clone()),
                    _ => None,
                })
                .unwrap_or_else(|| lbl.clone());
            let properties = match row.get(2) {
                Some(ExecValue::Map(m)) => {
                    let mut obj = serde_json::Map::new();
                    for (k, v) in m {
                        let json_val = exec_value_to_json(v);
                        obj.insert(k.clone(), json_val);
                    }
                    Value::Object(obj)
                }
                _ => json!({}),
            };
            all_rows.push((node_id, row_label, properties));
        }
        if all_rows.len() >= limit + offset {
            break;
        }
    }

    // Deduplicate by node_id (in case subclass expansion includes duplicates)
    let mut seen_ids = std::collections::HashSet::new();
    let mut entities = Vec::new();
    for (node_id, row_label, properties) in all_rows {
        if seen_ids.insert(node_id) {
            entities.push(json!({
                "node_id": node_id.to_string(),
                "label": row_label,
                "properties": properties,
            }));
        }
    }

    // Apply limit/offset after dedup
    let entities: Vec<_> = entities.into_iter().skip(offset).take(limit).collect();

    // Step 6: return
    Ok(json!({
        "content": [{
            "type": "text",
            "text": serde_json::to_string(&json!({
                "entities": entities,
            })).unwrap_or_default()
        }]
    }))
}

// ── explain_symbol ────────────────────────────────────────────────────────────

pub fn explain_symbol(db: &GraphDb, params: Option<Value>) -> Result<Value, Value> {
    let args = params.unwrap_or(json!({}));

    let name = args["name"]
        .as_str()
        .ok_or_else(|| mcp_error(-32602, "Missing required param: name", json!({})))?;
    let kind_str = args["kind"].as_str().unwrap_or("class");

    match kind_str {
        "class" => explain_class(db, name),
        "relation" => explain_relation(db, name),
        other => Err(mcp_error(
            -32602,
            "Invalid kind",
            json!({"detail": format!("kind must be 'class' or 'relation', got '{other}'")}),
        )),
    }
}

fn explain_class(db: &GraphDb, name: &str) -> Result<Value, Value> {
    // Step 1: resolve
    let resolved = resolve(db, name, AliasKind::Class)
        .map_err(|e| so_error_to_mcp_error(-32602, "Resolution failed", &e))?;
    let canonical = &resolved.canonical_name;
    let safe_c = escape_cypher_string(canonical);

    // Step 2: aliases
    let aliases = query_string_list(
        db,
        &format!(
            "MATCH (a:{ALIAS_LABEL})-[:{ALIAS_OF_REL}]->(c:{CLASS_LABEL} {{name: '{safe_c}'}}) \
             RETURN a.name"
        ),
    )?;

    // Step 3: direct subclasses
    let subclasses = query_string_list(
        db,
        &format!(
            "MATCH (sub:{CLASS_LABEL})-[:{SUBCLASS_OF_REL}]->(c:{CLASS_LABEL} {{name: '{safe_c}'}}) \
             RETURN sub.name"
        ),
    )?;

    // Step 4: parent classes
    let parent_classes = query_string_list(
        db,
        &format!(
            "MATCH (c:{CLASS_LABEL} {{name: '{safe_c}'}})-[:{SUBCLASS_OF_REL}]->(p:{CLASS_LABEL}) \
             RETURN p.name"
        ),
    )?;

    // Step 5: properties via HAS_PROPERTY
    let properties = {
        let q = format!(
            "MATCH (c:{CLASS_LABEL} {{name: '{safe_c}'}})-[:{HAS_PROPERTY_REL}]->(p:{PROPERTY_LABEL}) \
             RETURN p.name, p.datatype, p.required"
        );
        let result = execute_or_empty(db, &q)?;
        let mut out = Vec::new();
        for row in &result.rows {
            out.push(json!({
                "name": str_val(row, 0),
                "datatype": str_val(row, 1),
                "required": int_val(row, 2) != 0,
            }));
        }
        out
    };

    // Step 6: valid_relations_as_source (DOMAIN edge points to this class or its subclasses)
    let all_class_names = expand_subclasses(db, canonical, 20)
        .map_err(|e| mcp_error(-32603, "Subclass expansion failed", so_error_to_mcp(&e)))?;
    let class_list: Vec<String> = all_class_names
        .iter()
        .map(|n| format!("'{}'", escape_cypher_string(n)))
        .collect();
    let class_list_str = class_list.join(", ");

    let valid_relations_as_source = query_string_list(
        db,
        &format!(
            "MATCH (r:{RELATION_LABEL})-[:{DOMAIN_REL}]->(c:{CLASS_LABEL}) \
             WHERE c.name IN [{class_list_str}] RETURN r.name"
        ),
    )?;

    // Step 7: valid_relations_as_target (RANGE edge)
    let valid_relations_as_target = query_string_list(
        db,
        &format!(
            "MATCH (r:{RELATION_LABEL})-[:{RANGE_REL}]->(c:{CLASS_LABEL}) \
             WHERE c.name IN [{class_list_str}] RETURN r.name"
        ),
    )?;

    // Step 8: instance count
    let instance_count = {
        let q = format!("MATCH (n:{safe_c}) RETURN count(n)");
        match db.execute(&q) {
            Ok(r) => r
                .rows
                .first()
                .and_then(|row| row.first())
                .and_then(|v| if let ExecValue::Int64(n) = v { Some(*n) } else { None })
                .unwrap_or(0),
            Err(_) => 0,
        }
    };

    let result = json!({
        "kind": "class",
        "canonical_name": canonical,
        "symbol_id": resolved.symbol_id,
        "was_alias": resolved.was_alias,
        "original_name": resolved.original_name,
        "aliases": aliases,
        "subclasses": subclasses,
        "parent_classes": parent_classes,
        "properties": properties,
        "valid_relations_as_source": valid_relations_as_source,
        "valid_relations_as_target": valid_relations_as_target,
        "instance_count": instance_count,
    });

    Ok(json!({
        "content": [{
            "type": "text",
            "text": serde_json::to_string(&result).unwrap_or_default()
        }]
    }))
}

fn explain_relation(db: &GraphDb, name: &str) -> Result<Value, Value> {
    // Step 1: resolve
    let resolved = resolve(db, name, AliasKind::Relation)
        .map_err(|e| so_error_to_mcp_error(-32602, "Resolution failed", &e))?;
    let canonical = &resolved.canonical_name;
    let safe_r = escape_cypher_string(canonical);

    // Step 2: aliases
    let aliases = query_string_list(
        db,
        &format!(
            "MATCH (a:{ALIAS_LABEL})-[:{ALIAS_OF_REL}]->(r:{RELATION_LABEL} {{name: '{safe_r}'}}) \
             RETURN a.name"
        ),
    )?;

    // Parent relations (SUBPROPERTY_OF)
    let parent_relations = query_string_list(
        db,
        &format!(
            "MATCH (r:{RELATION_LABEL} {{name: '{safe_r}'}})-[:{SUBPROPERTY_OF_REL}]->(p:{RELATION_LABEL}) \
             RETURN p.name"
        ),
    )?;

    // Transitive sub-relations (BFS via __SO_SUBPROPERTY_OF, excludes self)
    let subproperties = {
        let mut all = expand_subproperties(db, canonical, 20)
            .map_err(|e| mcp_error(-32603, "Subproperty expansion failed", so_error_to_mcp(&e)))?;
        all.retain(|n| n != canonical);
        all
    };

    // Domain class name
    let domain_class = {
        let q = format!(
            "MATCH (r:{RELATION_LABEL} {{name: '{safe_r}'}})-[:{DOMAIN_REL}]->(c:{CLASS_LABEL}) \
             RETURN c.name"
        );
        let result = execute_or_empty(db, &q)?;
        result
            .rows
            .first()
            .and_then(|row| row.first())
            .and_then(|v| if let ExecValue::String(s) = v { Some(s.clone()) } else { None })
            .unwrap_or_default()
    };

    // Range class name
    let range_class = {
        let q = format!(
            "MATCH (r:{RELATION_LABEL} {{name: '{safe_r}'}})-[:{RANGE_REL}]->(c:{CLASS_LABEL}) \
             RETURN c.name"
        );
        let result = execute_or_empty(db, &q)?;
        result
            .rows
            .first()
            .and_then(|row| row.first())
            .and_then(|v| if let ExecValue::String(s) = v { Some(s.clone()) } else { None })
            .unwrap_or_default()
    };

    // expand_subclasses on domain and range
    let valid_source_classes = if domain_class.is_empty() {
        vec![]
    } else {
        expand_subclasses(db, &domain_class, 20)
            .map_err(|e| mcp_error(-32603, "Domain subclass expansion failed", so_error_to_mcp(&e)))?
    };

    let valid_target_classes = if range_class.is_empty() {
        vec![]
    } else {
        expand_subclasses(db, &range_class, 20)
            .map_err(|e| mcp_error(-32603, "Range subclass expansion failed", so_error_to_mcp(&e)))?
    };

    // Instance count: try MATCH ()-[r:REL_NAME]->() RETURN count(r)
    let instance_count = {
        let q = format!("MATCH ()-[r:{safe_r}]->() RETURN count(r)");
        match db.execute(&q) {
            Ok(r) => r
                .rows
                .first()
                .and_then(|row| row.first())
                .and_then(|v| if let ExecValue::Int64(n) = v { Some(*n) } else { None })
                .unwrap_or(0),
            Err(_) => 0,
        }
    };

    let result = json!({
        "kind": "relation",
        "canonical_name": canonical,
        "symbol_id": resolved.symbol_id,
        "was_alias": resolved.was_alias,
        "original_name": resolved.original_name,
        "aliases": aliases,
        "parent_relations": parent_relations,
        "subproperties": subproperties,
        "domain": domain_class,
        "range": range_class,
        "valid_source_classes": valid_source_classes,
        "valid_target_classes": valid_target_classes,
        "instance_count": instance_count,
    });

    Ok(json!({
        "content": [{
            "type": "text",
            "text": serde_json::to_string(&result).unwrap_or_default()
        }]
    }))
}

// ── validate ──────────────────────────────────────────────────────────────────

pub fn validate(db: &GraphDb, params: Option<Value>) -> Result<Value, Value> {
    let args = params.unwrap_or(json!({}));
    let scope = args["scope"].as_str().unwrap_or("full_graph");

    let start = Instant::now();
    let mut violations: Vec<Value> = Vec::new();
    let mut warnings: Vec<Value> = Vec::new();
    let mut nodes_scanned: u64 = 0;
    let mut edges_scanned: u64 = 0;

    // ── Step 1: Ontology consistency — every __SO_Relation must have DOMAIN and RANGE ──
    {
        let q = format!(
            "MATCH (r:{RELATION_LABEL}) RETURN r.name, r.symbol_id"
        );
        let result = execute_or_empty(db, &q)?;
        for row in &result.rows {
            let rel_name = str_val(row, 0);
            let rel_sid = str_val(row, 1);
            let safe_sid = escape_cypher_string(&rel_sid);

            // Check DOMAIN
            let domain_q = format!(
                "MATCH (r:{RELATION_LABEL} {{symbol_id: '{safe_sid}'}})-[:{DOMAIN_REL}]->(c:{CLASS_LABEL}) \
                 RETURN c.name"
            );
            let has_domain = execute_or_empty(db, &domain_q)?
                .rows
                .first()
                .is_some();
            if !has_domain {
                violations.push(json!({
                    "kind": "MissingDomain",
                    "message": format!("Relation '{}' has no DOMAIN edge", rel_name),
                    "relation": rel_name,
                }));
            }

            // Check RANGE
            let range_q = format!(
                "MATCH (r:{RELATION_LABEL} {{symbol_id: '{safe_sid}'}})-[:{RANGE_REL}]->(c:{CLASS_LABEL}) \
                 RETURN c.name"
            );
            let has_range = execute_or_empty(db, &range_q)?
                .rows
                .first()
                .is_some();
            if !has_range {
                violations.push(json!({
                    "kind": "MissingRange",
                    "message": format!("Relation '{}' has no RANGE edge", rel_name),
                    "relation": rel_name,
                }));
            }

            edges_scanned += 1;
        }
    }

    // ── Step 2: Full-graph scan (if requested) ────────────────────────────────
    if scope == "full_graph" {
        // Get all known canonical class names
        let known_classes: std::collections::HashSet<String> = {
            let q = format!("MATCH (c:{CLASS_LABEL}) RETURN c.name");
            let result = execute_or_empty(db, &q)?;
            result
                .rows
                .iter()
                .filter_map(|row| row.first())
                .filter_map(|v| if let ExecValue::String(s) = v { Some(s.clone()) } else { None })
                .collect()
        };

        // Get all known relation names to skip them when checking node labels
        // (CALL db.schema() returns both label names and relationship types as strings)
        let known_relations: std::collections::HashSet<String> = {
            let q = format!("MATCH (r:{RELATION_LABEL}) RETURN r.name");
            let result = execute_or_empty(db, &q)?;
            result
                .rows
                .iter()
                .filter_map(|row| row.first())
                .filter_map(|v| if let ExecValue::String(s) = v { Some(s.clone()) } else { None })
                .collect()
        };

        // Use CALL db.schema() to get all labels
        // If not available, fall back to scanning via MATCH (n) RETURN DISTINCT labels(n)[0]
        let all_labels: Vec<String> = {
            match db.execute("CALL db.schema()") {
                Ok(r) => {
                    // schema() returns rows; try to collect string values
                    r.rows
                        .iter()
                        .flat_map(|row| row.iter())
                        .filter_map(|v| if let ExecValue::String(s) = v { Some(s.clone()) } else { None })
                        .filter(|s| !s.is_empty())
                        .collect()
                }
                Err(_) => {
                    // Fall back: get distinct labels from all nodes
                    // labels(n) returns List([String(label)]) — no subscript support
                    match execute_or_empty(
                        db,
                        "MATCH (n) RETURN DISTINCT labels(n)",
                    ) {
                        Ok(r) => {
                            let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
                            for row in &r.rows {
                                if let Some(v) = row.first() {
                                    let label = match v {
                                        ExecValue::List(list) => list.first().and_then(|item| {
                                            if let ExecValue::String(s) = item { Some(s.clone()) } else { None }
                                        }),
                                        ExecValue::String(s) => Some(s.clone()),
                                        _ => None,
                                    };
                                    if let Some(l) = label {
                                        if !l.is_empty() {
                                            seen.insert(l);
                                        }
                                    }
                                }
                            }
                            seen.into_iter().collect()
                        },
                        Err(_) => vec![],
                    }
                }
            }
        };

        // Labels to always skip: __SO_ internal labels, and schema metadata labels
        // returned by CALL db.schema() (e.g. "node", "relationship")
        let schema_meta_labels: std::collections::HashSet<&str> =
            ["node", "relationship", "label", "property", "index", "constraint"]
                .iter()
                .cloned()
                .collect();

        for raw_label in &all_labels {
            // Skip __SO_ internal labels
            if raw_label.starts_with("__SO_") {
                continue;
            }
            // Skip schema metadata labels from CALL db.schema()
            if schema_meta_labels.contains(raw_label.as_str()) {
                continue;
            }
            // Skip relation types — CALL db.schema() returns both node labels and rel types
            if known_relations.contains(raw_label) {
                continue;
            }
            nodes_scanned += 1;

            // Check that the label is a known canonical class or resolvable via alias
            if !known_classes.contains(raw_label) {
                // Try resolve
                match resolve(db, raw_label, AliasKind::Class) {
                    Ok(_) => {
                        // Resolvable via alias — note as warning
                        warnings.push(json!({
                            "message": format!(
                                "Label '{}' is an alias, not a canonical class name",
                                raw_label
                            ),
                            "label": raw_label,
                        }));
                    }
                    Err(_) => {
                        violations.push(json!({
                            "kind": "UnknownClass",
                            "message": format!(
                                "Label '{}' is not a known class or alias in the ontology",
                                raw_label
                            ),
                            "label": raw_label,
                        }));
                    }
                }
            }
        }
    }

    // ── Step 3: per-class unseeded warning ────────────────────────────────────
    // When a specific class_name is provided and validation passes (no violations),
    // warn if the class has 0 declared properties — calling create_entity with any
    // properties will be rejected until add_property has been called.
    if violations.is_empty() {
        if let Some(class_name) = args["class_name"].as_str() {
            if !class_name.is_empty() {
                match resolve(db, class_name, AliasKind::Class) {
                    Ok(resolved) => {
                        let safe_sid = escape_cypher_string(&resolved.symbol_id);
                        let prop_q = format!(
                            "MATCH (c:{CLASS_LABEL} {{symbol_id: '{safe_sid}'}})-[:{HAS_PROPERTY_REL}]->(p:{PROPERTY_LABEL}) \
                             RETURN p.name"
                        );
                        let prop_count = execute_or_empty(db, &prop_q)
                            .map(|r| r.rows.len())
                            .unwrap_or(0);
                        if prop_count == 0 {
                            warnings.push(json!(
                                format!(
                                    "{} has 0 declared properties. create_entity will reject any \
                                     properties until add_property is called. This validation result \
                                     does not guarantee create_entity will succeed.",
                                    resolved.canonical_name
                                )
                            ));
                        }
                    }
                    Err(_) => {
                        // Unresolvable class name — leave it; violations will catch this
                        // if full_graph scan ran, or silently skip if scope was narrower.
                    }
                }
            }
        }
    }

    let duration_ms = start.elapsed().as_millis() as u64;
    let violations_found = violations.len() as u64;
    let warnings_found = warnings.len() as u64;

    let report = json!({
        "valid": violations.is_empty(),
        "violations": violations,
        "warnings": warnings,
        "stats": {
            "nodes_scanned": nodes_scanned,
            "edges_scanned": edges_scanned,
            "violations_found": violations_found,
            "warnings_found": warnings_found,
            "duration_ms": duration_ms,
        }
    });

    Ok(json!({
        "content": [{
            "type": "text",
            "text": serde_json::to_string(&report).unwrap_or_default()
        }]
    }))
}

// ── Query helpers ─────────────────────────────────────────────────────────────

/// Execute a query and return all first-column string values.
fn query_string_list(db: &GraphDb, q: &str) -> Result<Vec<String>, Value> {
    let result = execute_or_empty(db, q)?;
    let mut out = Vec::new();
    for row in &result.rows {
        if let Some(ExecValue::String(s)) = row.first() {
            out.push(s.clone());
        }
    }
    Ok(out)
}

/// Convert an ExecValue to a JSON Value for property serialization.
fn exec_value_to_json(v: &ExecValue) -> Value {
    match v {
        ExecValue::String(s) => json!(s),
        ExecValue::Int64(n) => json!(n),
        ExecValue::Float64(f) => json!(f),
        ExecValue::Bool(b) => json!(b),
        ExecValue::Null => json!(null),
        ExecValue::Map(m) => {
            let mut obj = serde_json::Map::new();
            for (k, inner) in m {
                obj.insert(k.clone(), exec_value_to_json(inner));
            }
            Value::Object(obj)
        }
        ExecValue::List(l) => {
            Value::Array(l.iter().map(exec_value_to_json).collect())
        }
        _ => json!(null),
    }
}
