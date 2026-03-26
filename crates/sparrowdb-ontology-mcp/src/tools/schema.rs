use std::collections::HashMap;

use serde_json::{json, Value};
use sparrowdb::GraphDb;
use sparrowdb_common::NodeId;
use sparrowdb_execution::Value as ExecValue;
use sparrowdb_ontology_core::model::AliasKind;
use sparrowdb_ontology_core::namespace::{
    ALIAS_LABEL, ALIAS_OF_REL, CLASS_LABEL, DOMAIN_REL, HAS_PROPERTY_REL, PROPERTY_LABEL,
    RANGE_REL, RELATION_LABEL, SUBCLASS_OF_REL, SUBPROPERTY_OF_REL,
};
use sparrowdb_ontology_core::{add_alias, add_property, define_subclass, init, resolve, StarterKind};
use sparrowdb_ontology_core::{DomainRangeStrategy, ImportOptions};
use sparrowdb_storage::node_store::Value as StoreValue;

use crate::error::{mcp_error, so_error_to_mcp};

// ── Storage value helpers ─────────────────────────────────────────────────────

fn sv(s: &str) -> StoreValue {
    StoreValue::Bytes(s.as_bytes().to_vec())
}

fn iv(n: i64) -> StoreValue {
    StoreValue::Int64(n)
}

fn bv(b: bool) -> StoreValue {
    StoreValue::Int64(if b { 1 } else { 0 })
}

fn props(pairs: &[(&str, StoreValue)]) -> HashMap<String, StoreValue> {
    pairs
        .iter()
        .map(|(k, v): &(&str, StoreValue)| (k.to_string(), v.clone()))
        .collect()
}

fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before Unix epoch")
        .as_millis() as i64
}

// ── Error handling helpers ────────────────────────────────────────────────────

/// Execute a query, treating "unknown label" / "unknown relationship type" as an empty result.
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

fn execute_params_or_empty(
    db: &GraphDb,
    q: &str,
    params: HashMap<String, ExecValue>,
) -> Result<sparrowdb_execution::QueryResult, Value> {
    match db.execute_with_params(q, params) {
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

// ── Dispatch ──────────────────────────────────────────────────────────────────

pub fn dispatch(db: &GraphDb, name: &str, params: Option<Value>) -> Result<Value, Value> {
    match name {
        "start_here" => start_here(db, params),
        "init" => tool_init(db, params),
        "get_ontology" => get_ontology(db, params),
        "define_class" => define_class(db, params),
        "define_relation" => define_relation(db, params),
        "add_alias" => tool_add_alias(db, params),
        "define_subclass" => tool_define_subclass(db, params),
        "define_subproperty" => define_subproperty(db, params),
        "resolve_name" => tool_resolve_name(db, params),
        "add_property" => tool_add_property(db, params),
        "health" => health(db, params),
        "stats" => stats(db, params),
        "export_json_ld" => tool_export_json_ld(db, params),
        _ => Err(mcp_error(-32601, "Method not found", json!({"tool": name}))),
    }
}

// ── init ──────────────────────────────────────────────────────────────────────

pub fn tool_init(db: &GraphDb, params: Option<Value>) -> Result<Value, Value> {
    let args = params.unwrap_or(json!({}));
    let force = args["force"].as_bool().unwrap_or(false);

    let (starter, starter_name) = match args["starter"].as_str().unwrap_or("WorldModel") {
        "Blank" | "blank" => (StarterKind::Blank, "Blank"),
        "PersonalKnowledge" | "personal_knowledge" => (StarterKind::PersonalKnowledge, "PersonalKnowledge"),
        "ProfessionalNetwork" | "professional_network" => (StarterKind::ProfessionalNetwork, "ProfessionalNetwork"),
        "ResearchNotes" | "research_notes" => (StarterKind::ResearchNotes, "ResearchNotes"),
        _ => (StarterKind::WorldModel, "WorldModel"),
    };

    let result = init(db, Some(starter), force)
        .map_err(|e| mcp_error(-32603, "Init failed", json!({"detail": e.to_string()})))?;

    Ok(json!({
        "content": [{
            "type": "text",
            "text": serde_json::to_string(&json!({
                "initialized": true,
                "starter": starter_name,
                "classes_created": result.classes_created,
                "relations_created": result.relations_created,
                "properties_created": result.properties_created,
                "next_steps": [
                    "Call start_here to see schema state and unseeded classes.",
                    "Call get_ontology to view all defined classes and relations.",
                    "Before calling create_entity with properties, declare each field via add_property(owner='ClassName', name='fieldName')."
                ]
            })).unwrap_or_default()
        }]
    }))
}

// ── start_here ────────────────────────────────────────────────────────────────

pub fn start_here(db: &GraphDb, _params: Option<Value>) -> Result<Value, Value> {
    // Detect init state: query count of __SO_Class nodes.
    // "unknown label" → uninitialized.
    let q = format!("MATCH (n:{CLASS_LABEL}) RETURN count(n)");
    let (initialized, class_count) = match db.execute(&q) {
        Ok(result) => {
            let count = result
                .rows
                .first()
                .and_then(|r| r.first())
                .map(|v| match v {
                    ExecValue::Int64(n) => *n,
                    _ => 0,
                })
                .unwrap_or(0);
            (count > 0, count)
        }
        Err(sparrowdb_common::Error::InvalidArgument(ref msg)) if msg.contains("unknown label") => {
            (false, 0)
        }
        Err(e) => {
            return Err(mcp_error(
                -32603,
                "Database error",
                json!({"detail": e.to_string()}),
            ))
        }
    };

    if initialized {
        // Relation count
        let rel_count = {
            let q2 = format!("MATCH (n:{RELATION_LABEL}) RETURN count(n)");
            match execute_or_empty(db, &q2) {
                Ok(r) => r
                    .rows
                    .first()
                    .and_then(|row| row.first())
                    .map(|v| match v {
                        ExecValue::Int64(n) => *n,
                        _ => 0,
                    })
                    .unwrap_or(0),
                Err(_) => 0,
            }
        };

        // Total declared property count
        let property_count = {
            let q3 = format!("MATCH (p:{PROPERTY_LABEL}) RETURN count(p)");
            match execute_or_empty(db, &q3) {
                Ok(r) => r
                    .rows
                    .first()
                    .and_then(|row| row.first())
                    .map(|v| match v {
                        ExecValue::Int64(n) => *n,
                        _ => 0,
                    })
                    .unwrap_or(0),
                Err(_) => 0,
            }
        };

        // All class names
        let all_class_names: Vec<String> = {
            let q4 = format!("MATCH (c:{CLASS_LABEL}) RETURN c.name");
            match execute_or_empty(db, &q4) {
                Ok(r) => r.rows.iter().map(|row| str_val(row, 0)).collect(),
                Err(_) => vec![],
            }
        };

        // Class names that have at least one declared property
        let seeded_names: std::collections::HashSet<String> = {
            let q5 = format!(
                "MATCH (c:{CLASS_LABEL})-[:{HAS_PROPERTY_REL}]->(p:{PROPERTY_LABEL}) RETURN c.name"
            );
            match execute_or_empty(db, &q5) {
                Ok(r) => r
                    .rows
                    .iter()
                    .map(|row| str_val(row, 0))
                    .collect(),
                Err(_) => std::collections::HashSet::new(),
            }
        };

        // Classes with 0 declared properties — sorted for stable output
        let mut unseeded_classes: Vec<String> = all_class_names
            .into_iter()
            .filter(|name| !seeded_names.contains(name))
            .collect();
        unseeded_classes.sort();

        let schema_seeding = if unseeded_classes.is_empty() {
            json!({ "status": "complete", "unseeded_classes": [] })
        } else {
            json!({
                "status": "incomplete",
                "unseeded_classes": unseeded_classes,
                "warning": "create_entity will reject any properties on these classes until add_property is called. Call create_entity with an empty properties object {} to create bare nodes without properties."
            })
        };

        Ok(json!({
            "content": [{
                "type": "text",
                "text": serde_json::to_string(&json!({
                    "status": "initialized",
                    "ontology": {
                        "class_count": class_count,
                        "relation_count": rel_count,
                        "property_count": property_count,
                    },
                    "schema_seeding": schema_seeding,
                    "schema_first_rule": "Properties must be declared via add_property before create_entity can store them. Params: owner (class name), name (property name), datatype (default: 'string'), required (default: false).",
                    "next_steps": [
                        "Call get_ontology to view all defined classes, relations, and their declared properties.",
                        "To use create_entity with properties: first call add_property(owner='ClassName', name='fieldName') for each field, then call create_entity.",
                        "Call define_class to add a new class.",
                        "Call define_relation to add a new relation.",
                        "Call create_relationship to create a typed edge between two entity node IDs.",
                    ]
                })).unwrap_or_default()
            }]
        }))
    } else {
        Ok(json!({
            "content": [{
                "type": "text",
                "text": serde_json::to_string(&json!({
                    "status": "uninitialized",
                    "message": "The ontology has not been initialized yet.",
                    "starter_templates": {
                        "world_model": "Canonical professional world model — Person, Organization, Project, Task, Role, Event, Decision, Policy, Concept, Dependency (10 classes, 19 relations, 22 properties). Good general-purpose starting point.",
                        "personal_knowledge": "Personal knowledge graph — Person, Concept, Event, Location, Document (5 classes, 5 relations). Good for personal notes, journaling, and idea networks.",
                        "professional_network": "Professional network — Person, Organization, Role, Project, Event (5 classes, 6 relations). Good for contact management and career tracking.",
                        "research_notes": "Research notes — Concept, Document, Claim, Person, Asset (5 classes, 6 relations). Good for academic research, citation tracking, and claim validation.",
                        "blank": "Empty schema — no classes or relations seeded. Use define_class and define_relation to build from scratch.",
                    },
                    "next_steps": [
                        "Call the init tool with a starter parameter to bootstrap the ontology. Options: 'world_model' (default), 'personal_knowledge', 'professional_network', 'research_notes', or 'blank'.",
                        "After init, call get_ontology to inspect the schema.",
                        "Then call define_class / define_relation to extend the schema.",
                        "Before calling create_entity with properties, declare each field via add_property(owner='ClassName', name='fieldName').",
                    ]
                })).unwrap_or_default()
            }]
        }))
    }
}

// ── get_ontology ──────────────────────────────────────────────────────────────

pub fn get_ontology(db: &GraphDb, _params: Option<Value>) -> Result<Value, Value> {
    // Query all classes
    let classes = {
        let q = format!(
            "MATCH (c:{CLASS_LABEL}) RETURN c.symbol_id, c.name, c.description, c.status, c.created_at, c.updated_at, c.iri"
        );
        let result = execute_or_empty(db, &q)?;
        let mut out = Vec::new();
        for row in &result.rows {
            let symbol_id = str_val(&row, 0);
            let name = str_val(&row, 1);
            let description = str_val_opt(&row, 2);
            let status = str_val(&row, 3);
            let created_at = int_val(&row, 4);
            let updated_at = int_val(&row, 5);
            let iri = str_val_opt(&row, 6);

            // Aliases for this class
            let aliases = get_aliases_for(db, &name, "class")?;
            // Direct subclasses
            let subclasses = get_direct_subclasses(db, &name)?;
            // Properties
            let properties = get_properties_for_class(db, &symbol_id)?;

            out.push(json!({
                "symbol_id": symbol_id,
                "name": name,
                "description": description,
                "status": status,
                "iri": iri,
                "created_at": created_at,
                "updated_at": updated_at,
                "aliases": aliases,
                "subclasses": subclasses,
                "properties": properties,
            }));
        }
        out
    };

    // Query all relations
    let relations = {
        let q = format!(
            "MATCH (r:{RELATION_LABEL}) RETURN r.symbol_id, r.name, r.description, r.status, r.directed, r.created_at, r.updated_at, r.iri"
        );
        let result = execute_or_empty(db, &q)?;
        let mut out = Vec::new();
        for row in &result.rows {
            let symbol_id = str_val(&row, 0);
            let name = str_val(&row, 1);
            let description = str_val_opt(&row, 2);
            let status = str_val(&row, 3);
            let directed = int_val(&row, 4) != 0;
            let created_at = int_val(&row, 5);
            let updated_at = int_val(&row, 6);
            let iri = str_val_opt(&row, 7);

            let domain = get_domain_for_relation(db, &name)?;
            let range = get_range_for_relation(db, &name)?;
            let aliases = get_aliases_for(db, &name, "relation")?;

            out.push(json!({
                "symbol_id": symbol_id,
                "name": name,
                "description": description,
                "status": status,
                "directed": directed,
                "iri": iri,
                "domain": domain,
                "range": range,
                "created_at": created_at,
                "updated_at": updated_at,
                "aliases": aliases,
            }));
        }
        out
    };

    // Query all aliases
    let aliases = {
        let q = format!(
            "MATCH (a:{ALIAS_LABEL}) RETURN a.name, a.kind, a.target_name, a.created_at"
        );
        let result = execute_or_empty(db, &q)?;
        let mut out = Vec::new();
        for row in &result.rows {
            out.push(json!({
                "name": str_val(&row, 0),
                "kind": str_val(&row, 1),
                "target_name": str_val(&row, 2),
                "created_at": int_val(&row, 3),
            }));
        }
        out
    };

    // Query all properties
    let properties = {
        let q = format!(
            "MATCH (p:{PROPERTY_LABEL}) RETURN p.symbol_id, p.name, p.datatype, p.required, p.owner_symbol_id, p.owner_kind, p.created_at"
        );
        let result = execute_or_empty(db, &q)?;
        let mut out = Vec::new();
        for row in &result.rows {
            out.push(json!({
                "symbol_id": str_val(&row, 0),
                "name": str_val(&row, 1),
                "datatype": str_val(&row, 2),
                "required": int_val(&row, 3) != 0,
                "owner_symbol_id": str_val(&row, 4),
                "owner_kind": str_val(&row, 5),
                "created_at": int_val(&row, 6),
            }));
        }
        out
    };

    let ontology = json!({
        "classes": classes,
        "relations": relations,
        "aliases": aliases,
        "properties": properties,
    });

    Ok(json!({
        "content": [{
            "type": "text",
            "text": serde_json::to_string(&ontology).unwrap_or_default()
        }]
    }))
}

// ── define_class ──────────────────────────────────────────────────────────────

pub fn define_class(db: &GraphDb, params: Option<Value>) -> Result<Value, Value> {
    let args = params.unwrap_or(json!({}));
    let name = args["name"]
        .as_str()
        .ok_or_else(|| mcp_error(-32602, "Missing required param: name", json!({})))?;
    let description = args["description"].as_str().unwrap_or("");
    let iri = args["iri"].as_str().unwrap_or("");

    // Validate: name must not start with __SO_
    if name.starts_with("__SO_") {
        return Err(mcp_error(
            -32602,
            "Reserved namespace",
            json!({"detail": format!("Class name '{name}' starts with reserved prefix '__SO_'.")}),
        ));
    }

    let symbol_id = uuid::Uuid::new_v4().to_string();
    let now = now_ms();

    let mut tx = db.begin_write().map_err(|e| {
        mcp_error(-32603, "Database error", json!({"detail": e.to_string()}))
    })?;

    tx.merge_node(
        CLASS_LABEL,
        props(&[
            ("symbol_id", sv(&symbol_id)),
            ("name", sv(name)),
            ("description", sv(description)),
            ("status", sv("active")),
            ("iri", sv(iri)),
            ("created_at", iv(now)),
            ("updated_at", iv(now)),
        ]),
    )
    .map_err(|e| mcp_error(-32603, "Failed to create class", json!({"detail": e.to_string()})))?;

    tx.commit().map_err(|e| {
        mcp_error(-32603, "Failed to commit", json!({"detail": e.to_string()}))
    })?;

    let iri_opt: Option<&str> = if iri.is_empty() { None } else { Some(iri) };
    let created = json!({
        "symbol_id": symbol_id,
        "name": name,
        "description": description,
        "status": "active",
        "iri": iri_opt,
        "created_at": now,
        "updated_at": now,
    });

    Ok(json!({
        "content": [{
            "type": "text",
            "text": serde_json::to_string(&json!({"created": created})).unwrap_or_default()
        }]
    }))
}

// ── define_relation ───────────────────────────────────────────────────────────

pub fn define_relation(db: &GraphDb, params: Option<Value>) -> Result<Value, Value> {
    let args = params.unwrap_or(json!({}));
    let name = args["name"]
        .as_str()
        .ok_or_else(|| mcp_error(-32602, "Missing required param: name", json!({})))?;
    let domain_name = args["domain"]
        .as_str()
        .ok_or_else(|| mcp_error(-32602, "Missing required param: domain", json!({})))?;
    let range_name = args["range"]
        .as_str()
        .ok_or_else(|| mcp_error(-32602, "Missing required param: range", json!({})))?;
    let description = args["description"].as_str().unwrap_or("");
    let directed = args["directed"].as_bool().unwrap_or(true);
    let iri = args["iri"].as_str().unwrap_or("");

    // Validate name
    if name.starts_with("__SO_") {
        return Err(mcp_error(
            -32602,
            "Reserved namespace",
            json!({"detail": format!("Relation name '{name}' starts with reserved prefix '__SO_'.")}),
        ));
    }

    // Resolve domain and range via core resolve()
    let domain_sym = resolve(db, domain_name, AliasKind::Class)
        .map_err(|e| mcp_error(-32602, "Domain resolution failed", so_error_to_mcp(&e)))?;
    let range_sym = resolve(db, range_name, AliasKind::Class)
        .map_err(|e| mcp_error(-32602, "Range resolution failed", so_error_to_mcp(&e)))?;

    let symbol_id = uuid::Uuid::new_v4().to_string();
    let now = now_ms();

    // Create the relation node
    let mut tx = db.begin_write().map_err(|e| {
        mcp_error(-32603, "Database error", json!({"detail": e.to_string()}))
    })?;

    let rel_node_id = tx
        .merge_node(
            RELATION_LABEL,
            props(&[
                ("symbol_id", sv(&symbol_id)),
                ("name", sv(name)),
                ("description", sv(description)),
                ("status", sv("active")),
                ("directed", bv(directed)),
                ("iri", sv(iri)),
                ("created_at", iv(now)),
                ("updated_at", iv(now)),
            ]),
        )
        .map_err(|e| {
            mcp_error(-32603, "Failed to create relation node", json!({"detail": e.to_string()}))
        })?;

    tx.commit().map_err(|e| {
        mcp_error(-32603, "Failed to commit relation node", json!({"detail": e.to_string()}))
    })?;

    // Fetch domain and range node IDs by symbol_id
    let domain_node_id = get_node_id_by_symbol_id(db, CLASS_LABEL, &domain_sym.symbol_id)?;
    let range_node_id = get_node_id_by_symbol_id(db, CLASS_LABEL, &range_sym.symbol_id)?;

    // Create DOMAIN and RANGE edges
    let mut tx2 = db.begin_write().map_err(|e| {
        mcp_error(-32603, "Database error", json!({"detail": e.to_string()}))
    })?;
    tx2.create_edge(rel_node_id, domain_node_id, DOMAIN_REL, HashMap::new())
        .map_err(|e| {
            mcp_error(-32603, "Failed to create DOMAIN edge", json!({"detail": e.to_string()}))
        })?;
    tx2.create_edge(rel_node_id, range_node_id, RANGE_REL, HashMap::new())
        .map_err(|e| {
            mcp_error(-32603, "Failed to create RANGE edge", json!({"detail": e.to_string()}))
        })?;
    tx2.commit().map_err(|e| {
        mcp_error(-32603, "Failed to commit edges", json!({"detail": e.to_string()}))
    })?;

    let iri_opt: Option<&str> = if iri.is_empty() { None } else { Some(iri) };
    let created = json!({
        "symbol_id": symbol_id,
        "name": name,
        "description": description,
        "status": "active",
        "directed": directed,
        "iri": iri_opt,
        "domain": domain_sym.canonical_name,
        "range": range_sym.canonical_name,
        "created_at": now,
        "updated_at": now,
    });

    Ok(json!({
        "content": [{
            "type": "text",
            "text": serde_json::to_string(&json!({"created": created})).unwrap_or_default()
        }]
    }))
}

// ── add_alias ─────────────────────────────────────────────────────────────────

pub fn tool_add_alias(db: &GraphDb, params: Option<Value>) -> Result<Value, Value> {
    let args = params.unwrap_or(json!({}));
    let alias_name = args["alias_name"]
        .as_str()
        .ok_or_else(|| mcp_error(-32602, "Missing required param: alias_name", json!({})))?;
    let target = args["target"]
        .as_str()
        .ok_or_else(|| mcp_error(-32602, "Missing required param: target", json!({})))?;
    let kind_str = args["kind"].as_str().unwrap_or("class");

    let kind = match kind_str {
        "class" => AliasKind::Class,
        "relation" => AliasKind::Relation,
        other => {
            return Err(mcp_error(
                -32602,
                "Invalid kind",
                json!({"detail": format!("kind must be 'class' or 'relation', got '{other}'")}),
            ))
        }
    };

    add_alias(db, alias_name, kind, target)
        .map_err(|e| mcp_error(-32602, "add_alias failed", so_error_to_mcp(&e)))?;

    Ok(json!({
        "content": [{
            "type": "text",
            "text": serde_json::to_string(&json!({
                "success": true,
                "alias_name": alias_name,
                "target": target,
                "kind": kind_str,
            })).unwrap_or_default()
        }]
    }))
}

// ── define_subclass ───────────────────────────────────────────────────────────

pub fn tool_define_subclass(db: &GraphDb, params: Option<Value>) -> Result<Value, Value> {
    let args = params.unwrap_or(json!({}));
    let child = args["child"]
        .as_str()
        .ok_or_else(|| mcp_error(-32602, "Missing required param: child", json!({})))?;
    let parent = args["parent"]
        .as_str()
        .ok_or_else(|| mcp_error(-32602, "Missing required param: parent", json!({})))?;

    define_subclass(db, child, parent)
        .map_err(|e| mcp_error(-32602, "define_subclass failed", so_error_to_mcp(&e)))?;

    Ok(json!({
        "content": [{
            "type": "text",
            "text": serde_json::to_string(&json!({
                "success": true,
                "child": child,
                "parent": parent,
                "edge": SUBCLASS_OF_REL,
            })).unwrap_or_default()
        }]
    }))
}

// ── define_subproperty ────────────────────────────────────────────────────────

pub fn define_subproperty(db: &GraphDb, params: Option<Value>) -> Result<Value, Value> {
    let args = params.unwrap_or(json!({}));
    let child = args["child"]
        .as_str()
        .ok_or_else(|| mcp_error(-32602, "Missing required param: child", json!({})))?;
    let parent = args["parent"]
        .as_str()
        .ok_or_else(|| mcp_error(-32602, "Missing required param: parent", json!({})))?;

    // Resolve both as relations
    let child_sym = resolve(db, child, AliasKind::Relation)
        .map_err(|e| mcp_error(-32602, "child resolution failed", so_error_to_mcp(&e)))?;
    let parent_sym = resolve(db, parent, AliasKind::Relation)
        .map_err(|e| mcp_error(-32602, "parent resolution failed", so_error_to_mcp(&e)))?;

    // Cycle detection (using RELATION_LABEL BFS via check_no_cycle)
    // Note: check_no_cycle uses CLASS_LABEL internally, so we do our own BFS check here.
    check_no_subproperty_cycle(db, &child_sym.canonical_name, &parent_sym.canonical_name)?;

    // Get NodeIds
    let child_id = get_node_id_by_symbol_id(db, RELATION_LABEL, &child_sym.symbol_id)?;
    let parent_id = get_node_id_by_symbol_id(db, RELATION_LABEL, &parent_sym.symbol_id)?;

    let mut tx = db.begin_write().map_err(|e| {
        mcp_error(-32603, "Database error", json!({"detail": e.to_string()}))
    })?;
    tx.create_edge(child_id, parent_id, SUBPROPERTY_OF_REL, HashMap::new())
        .map_err(|e| {
            mcp_error(
                -32603,
                "Failed to create SUBPROPERTY_OF edge",
                json!({"detail": e.to_string()}),
            )
        })?;
    tx.commit().map_err(|e| {
        mcp_error(-32603, "Failed to commit", json!({"detail": e.to_string()}))
    })?;

    Ok(json!({
        "content": [{
            "type": "text",
            "text": serde_json::to_string(&json!({
                "success": true,
                "child": child_sym.canonical_name,
                "parent": parent_sym.canonical_name,
                "edge": SUBPROPERTY_OF_REL,
            })).unwrap_or_default()
        }]
    }))
}

/// BFS cycle check for SUBPROPERTY_OF edges on RELATION_LABEL nodes.
fn check_no_subproperty_cycle(db: &GraphDb, child: &str, parent: &str) -> Result<(), Value> {
    use std::collections::HashSet;
    let mut visited: HashSet<String> = HashSet::new();
    let mut frontier = vec![parent.to_string()];
    visited.insert(parent.to_string());

    for _ in 0..50 {
        if frontier.is_empty() {
            break;
        }
        let mut next = Vec::new();
        for name in &frontier {
            let q = format!(
                "MATCH (n:{RELATION_LABEL} {{name: $nname}})-[:{SUBPROPERTY_OF_REL}]->(p:{RELATION_LABEL}) RETURN p.name"
            );
            let p = HashMap::from([("nname".to_string(), ExecValue::String(name.clone()))]);
            let result = match db.execute_with_params(&q, p) {
                Ok(r) => r,
                Err(sparrowdb_common::Error::InvalidArgument(ref msg))
                    if msg.contains("unknown label")
                        || msg.contains("unknown relationship type") =>
                {
                    continue;
                }
                Err(e) => {
                    return Err(mcp_error(
                        -32603,
                        "Database error during cycle check",
                        json!({"detail": e.to_string()}),
                    ))
                }
            };
            for row in &result.rows {
                if let Some(ExecValue::String(p)) = row.first() {
                    if p == child {
                        return Err(mcp_error(
                            -32602,
                            "Cycle detected",
                            json!({
                                "error_kind": "CycleDetected",
                                "detail": format!("Adding '{child}' → '{parent}' would create a cycle"),
                                "child": child,
                                "parent": parent,
                            }),
                        ));
                    }
                    if visited.insert(p.clone()) {
                        next.push(p.clone());
                    }
                }
            }
        }
        frontier = next;
    }
    Ok(())
}

// ── resolve_name ──────────────────────────────────────────────────────────────

pub fn tool_resolve_name(db: &GraphDb, params: Option<Value>) -> Result<Value, Value> {
    let args = params.unwrap_or(json!({}));
    let name = args["name"]
        .as_str()
        .ok_or_else(|| mcp_error(-32602, "Missing required param: name", json!({})))?;
    let kind_str = args["kind"].as_str().unwrap_or("class");

    let kind = match kind_str {
        "class" => AliasKind::Class,
        "relation" => AliasKind::Relation,
        other => {
            return Err(mcp_error(
                -32602,
                "Invalid kind",
                json!({"detail": format!("kind must be 'class' or 'relation', got '{other}'")}),
            ))
        }
    };

    let resolved =
        resolve(db, name, kind.clone())
            .map_err(|e| mcp_error(-32602, "Resolution failed", so_error_to_mcp(&e)))?;

    // Fetch all aliases of the canonical
    let aliases = get_aliases_for(db, &resolved.canonical_name, kind_str)?;

    // Fetch subclasses or subproperties
    let hierarchy = match kind {
        AliasKind::Class => {
            let subs = get_direct_subclasses(db, &resolved.canonical_name)?;
            json!({
                "kind": "subclasses",
                "direct": subs,
            })
        }
        AliasKind::Relation => {
            let subs = get_direct_subproperties(db, &resolved.canonical_name)?;
            json!({
                "kind": "subproperties",
                "direct": subs,
            })
        }
    };

    let result = json!({
        "canonical_name": resolved.canonical_name,
        "symbol_id": resolved.symbol_id,
        "was_alias": resolved.was_alias,
        "original_name": resolved.original_name,
        "kind": kind_str,
        "aliases": aliases,
        "hierarchy": hierarchy,
    });

    Ok(json!({
        "content": [{
            "type": "text",
            "text": serde_json::to_string(&result).unwrap_or_default()
        }]
    }))
}

// ── add_property ──────────────────────────────────────────────────────────────

pub fn tool_add_property(db: &GraphDb, params: Option<Value>) -> Result<Value, Value> {
    let args = params.unwrap_or(json!({}));
    let owner = args["owner"]
        .as_str()
        .ok_or_else(|| mcp_error(-32602, "Missing required param: owner", json!({})))?;
    let name = args["name"]
        .as_str()
        .ok_or_else(|| mcp_error(-32602, "Missing required param: name", json!({})))?;
    let datatype = args["datatype"].as_str().unwrap_or("string");
    let required = args["required"].as_bool().unwrap_or(false);
    let unique = args["unique"].as_bool().unwrap_or(false);
    let allowed_values: Option<Vec<String>> = args["allowed_values"]
        .as_array()
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(str::to_string)).collect());

    let valid_types = ["string", "int64", "float64", "bool", "date", "variant"];
    if !valid_types.contains(&datatype) {
        return Err(mcp_error(
            -32602,
            "Invalid datatype",
            json!({
                "error_kind": "InvalidDatatype",
                "detail": format!("datatype '{datatype}' is not valid"),
                "valid_options": valid_types,
                "suggestion": "Use one of: string, int64, float64, bool, date, variant",
            }),
        ));
    }

    let prop = add_property(db, owner, name, datatype, required, unique, allowed_values)
        .map_err(|e| mcp_error(-32602, "add_property failed", so_error_to_mcp(&e)))?;

    Ok(json!({
        "content": [{
            "type": "text",
            "text": serde_json::to_string(&json!({
                "created": {
                    "symbol_id": prop.symbol_id,
                    "owner": prop.owner_name,
                    "name": prop.name,
                    "datatype": format!("{:?}", prop.datatype).to_lowercase(),
                    "required": prop.required,
                    "unique": prop.unique,
                    "allowed_values": prop.allowed_values,
                    "created_at": prop.created_at,
                }
            })).unwrap_or_default()
        }]
    }))
}

// ── Query helpers ─────────────────────────────────────────────────────────────

fn get_aliases_for(db: &GraphDb, canonical_name: &str, kind_str: &str) -> Result<Vec<String>, Value> {
    let target_label = if kind_str == "relation" { RELATION_LABEL } else { CLASS_LABEL };
    let q = format!(
        "MATCH (a:{ALIAS_LABEL})-[:{ALIAS_OF_REL}]->(c:{target_label} {{name: $cname}}) \
         WHERE a.kind = $kind RETURN a.name"
    );
    let result = execute_params_or_empty(
        db,
        &q,
        HashMap::from([
            ("cname".to_string(), ExecValue::String(canonical_name.to_string())),
            ("kind".to_string(), ExecValue::String(kind_str.to_string())),
        ]),
    )?;
    let mut names = Vec::new();
    for row in &result.rows {
        if let Some(ExecValue::String(s)) = row.first() {
            names.push(s.clone());
        }
    }
    Ok(names)
}

fn get_direct_subclasses(db: &GraphDb, class_name: &str) -> Result<Vec<String>, Value> {
    let q = format!(
        "MATCH (sub:{CLASS_LABEL})-[:{SUBCLASS_OF_REL}]->(base:{CLASS_LABEL} {{name: $cname}}) RETURN sub.name"
    );
    let result = execute_params_or_empty(
        db,
        &q,
        HashMap::from([("cname".to_string(), ExecValue::String(class_name.to_string()))]),
    )?;
    let mut names = Vec::new();
    for row in &result.rows {
        if let Some(ExecValue::String(s)) = row.first() {
            names.push(s.clone());
        }
    }
    Ok(names)
}

fn get_direct_subproperties(db: &GraphDb, rel_name: &str) -> Result<Vec<String>, Value> {
    let q = format!(
        "MATCH (sub:{RELATION_LABEL})-[:{SUBPROPERTY_OF_REL}]->(base:{RELATION_LABEL} {{name: $rname}}) RETURN sub.name"
    );
    let result = execute_params_or_empty(
        db,
        &q,
        HashMap::from([("rname".to_string(), ExecValue::String(rel_name.to_string()))]),
    )?;
    let mut names = Vec::new();
    for row in &result.rows {
        if let Some(ExecValue::String(s)) = row.first() {
            names.push(s.clone());
        }
    }
    Ok(names)
}

fn get_domain_for_relation(db: &GraphDb, rel_name: &str) -> Result<Option<String>, Value> {
    let q = format!(
        "MATCH (r:{RELATION_LABEL} {{name: $rname}})-[:{DOMAIN_REL}]->(c:{CLASS_LABEL}) RETURN c.name"
    );
    let result = execute_params_or_empty(
        db,
        &q,
        HashMap::from([("rname".to_string(), ExecValue::String(rel_name.to_string()))]),
    )?;
    Ok(result
        .rows
        .first()
        .and_then(|row| row.first())
        .and_then(|v| if let ExecValue::String(s) = v { Some(s.clone()) } else { None }))
}

fn get_range_for_relation(db: &GraphDb, rel_name: &str) -> Result<Option<String>, Value> {
    let q = format!(
        "MATCH (r:{RELATION_LABEL} {{name: $rname}})-[:{RANGE_REL}]->(c:{CLASS_LABEL}) RETURN c.name"
    );
    let result = execute_params_or_empty(
        db,
        &q,
        HashMap::from([("rname".to_string(), ExecValue::String(rel_name.to_string()))]),
    )?;
    Ok(result
        .rows
        .first()
        .and_then(|row| row.first())
        .and_then(|v| if let ExecValue::String(s) = v { Some(s.clone()) } else { None }))
}

fn get_properties_for_class(db: &GraphDb, class_symbol_id: &str) -> Result<Vec<Value>, Value> {
    let q = format!(
        "MATCH (c:{CLASS_LABEL} {{symbol_id: $sid}})-[:{HAS_PROPERTY_REL}]->(p:{PROPERTY_LABEL}) \
         RETURN p.name, p.datatype, p.required"
    );
    let result = execute_params_or_empty(
        db,
        &q,
        HashMap::from([("sid".to_string(), ExecValue::String(class_symbol_id.to_string()))]),
    )?;
    let mut props = Vec::new();
    for row in &result.rows {
        props.push(json!({
            "name": str_val(&row, 0),
            "datatype": str_val(&row, 1),
            "required": int_val(&row, 2) != 0,
        }));
    }
    Ok(props)
}

fn get_node_id_by_symbol_id(
    db: &GraphDb,
    label: &str,
    symbol_id: &str,
) -> Result<NodeId, Value> {
    let q = format!("MATCH (n:{label} {{symbol_id: $sid}}) RETURN id(n)");
    let result = execute_params_or_empty(
        db,
        &q,
        HashMap::from([("sid".to_string(), ExecValue::String(symbol_id.to_string()))]),
    )?;
    result
        .rows
        .first()
        .and_then(|row| row.first())
        .and_then(|v| {
            if let ExecValue::Int64(n) = v {
                Some(NodeId(*n as u64))
            } else {
                None
            }
        })
        .ok_or_else(|| {
            mcp_error(
                -32603,
                "Node not found",
                json!({"label": label, "symbol_id": symbol_id}),
            )
        })
}

// ── Row value extraction helpers ──────────────────────────────────────────────

fn str_val(row: &[ExecValue], idx: usize) -> String {
    row.get(idx)
        .and_then(|v| if let ExecValue::String(s) = v { Some(s.clone()) } else { None })
        .unwrap_or_default()
}

fn str_val_opt(row: &[ExecValue], idx: usize) -> Option<String> {
    row.get(idx).and_then(|v| match v {
        ExecValue::String(s) if !s.is_empty() => Some(s.clone()),
        _ => None,
    })
}

fn int_val(row: &[ExecValue], idx: usize) -> i64 {
    row.get(idx)
        .and_then(|v| if let ExecValue::Int64(n) = v { Some(*n) } else { None })
        .unwrap_or(0)
}

// ── health ────────────────────────────────────────────────────────────────────

/// Return operational status: db connectivity, class/relation counts, db path.
pub fn health(db: &GraphDb, _params: Option<Value>) -> Result<Value, Value> {
    let q = format!("MATCH (n:{CLASS_LABEL}) RETURN count(n)");
    let (db_connected, class_count) = match db.execute(&q) {
        Ok(result) => {
            let count = result
                .rows
                .first()
                .and_then(|r| r.first())
                .map(|v| match v {
                    ExecValue::Int64(n) => *n,
                    _ => 0,
                })
                .unwrap_or(0);
            (true, count)
        }
        Err(sparrowdb_common::Error::InvalidArgument(ref msg)) if msg.contains("unknown label") => {
            // DB is open but ontology not yet initialized — still connected
            (true, 0)
        }
        Err(_) => (false, 0),
    };

    let relation_count = if db_connected && class_count > 0 {
        let q2 = format!("MATCH (n:{RELATION_LABEL}) RETURN count(n)");
        match execute_or_empty(db, &q2) {
            Ok(r) => r
                .rows
                .first()
                .and_then(|row| row.first())
                .map(|v| match v {
                    ExecValue::Int64(n) => *n,
                    _ => 0,
                })
                .unwrap_or(0),
            Err(_) => 0,
        }
    } else {
        0
    };

    let payload = json!({
        "status": "ok",
        "service": "sparrow-ontology-mcp",
        "db_connected": db_connected,
        "class_count": class_count,
        "relation_count": relation_count,
    });

    Ok(json!({
        "content": [{
            "type": "text",
            "text": serde_json::to_string(&payload).unwrap_or_default()
        }]
    }))
}

// ── stats ─────────────────────────────────────────────────────────────────────

/// Return ontology analytics: schema counts, unseeded classes, entity counts by class.
pub fn stats(db: &GraphDb, _params: Option<Value>) -> Result<Value, Value> {
    // --- Schema section ---
    let q_class = format!("MATCH (n:{CLASS_LABEL}) RETURN count(n)");
    let class_count = match db.execute(&q_class) {
        Ok(result) => result
            .rows
            .first()
            .and_then(|r| r.first())
            .map(|v| match v {
                ExecValue::Int64(n) => *n,
                _ => 0,
            })
            .unwrap_or(0),
        Err(sparrowdb_common::Error::InvalidArgument(ref msg)) if msg.contains("unknown label") => 0,
        Err(e) => {
            return Err(mcp_error(
                -32603,
                "Database error",
                json!({"detail": e.to_string()}),
            ))
        }
    };

    let relation_count = {
        let q = format!("MATCH (n:{RELATION_LABEL}) RETURN count(n)");
        match execute_or_empty(db, &q) {
            Ok(r) => r
                .rows
                .first()
                .and_then(|row| row.first())
                .map(|v| match v {
                    ExecValue::Int64(n) => *n,
                    _ => 0,
                })
                .unwrap_or(0),
            Err(_) => 0,
        }
    };

    let property_count = {
        let q = format!("MATCH (p:{PROPERTY_LABEL}) RETURN count(p)");
        match execute_or_empty(db, &q) {
            Ok(r) => r
                .rows
                .first()
                .and_then(|row| row.first())
                .map(|v| match v {
                    ExecValue::Int64(n) => *n,
                    _ => 0,
                })
                .unwrap_or(0),
            Err(_) => 0,
        }
    };

    // All class names
    let all_class_names: Vec<String> = {
        let q = format!("MATCH (c:{CLASS_LABEL}) RETURN c.name");
        match execute_or_empty(db, &q) {
            Ok(r) => r.rows.iter().map(|row| str_val(row, 0)).collect(),
            Err(_) => vec![],
        }
    };

    // Seeded class names (those with at least one declared property)
    let seeded_names: std::collections::HashSet<String> = {
        let q = format!(
            "MATCH (c:{CLASS_LABEL})-[:{HAS_PROPERTY_REL}]->(p:{PROPERTY_LABEL}) RETURN c.name"
        );
        match execute_or_empty(db, &q) {
            Ok(r) => r.rows.iter().map(|row| str_val(row, 0)).collect(),
            Err(_) => std::collections::HashSet::new(),
        }
    };

    let mut unseeded_classes: Vec<String> = all_class_names
        .iter()
        .filter(|name| !seeded_names.contains(*name))
        .cloned()
        .collect();
    unseeded_classes.sort();

    // --- Entities section ---
    let mut total_entities: i64 = 0;
    let mut by_class = serde_json::Map::new();

    for class_name in &all_class_names {
        let q = format!("MATCH (n:{class_name}) RETURN count(n)");
        let count = match db.execute(&q) {
            Ok(r) => r
                .rows
                .first()
                .and_then(|row| row.first())
                .map(|v| match v {
                    ExecValue::Int64(n) => *n,
                    _ => 0,
                })
                .unwrap_or(0),
            // "unknown label" means no entities of this class yet — treat as 0
            Err(sparrowdb_common::Error::InvalidArgument(ref msg))
                if msg.contains("unknown label") =>
            {
                0
            }
            // Any other error: skip this class silently
            Err(_) => 0,
        };
        total_entities += count;
        by_class.insert(class_name.clone(), json!(count));
    }

    let payload = json!({
        "schema": {
            "class_count": class_count,
            "relation_count": relation_count,
            "property_count": property_count,
            "unseeded_classes": unseeded_classes,
        },
        "entities": {
            "total": total_entities,
            "by_class": by_class,
        }
    });

    Ok(json!({
        "content": [{
            "type": "text",
            "text": serde_json::to_string(&payload).unwrap_or_default()
        }]
    }))
}

// ── export_json_ld ────────────────────────────────────────────────────────────

pub fn tool_export_json_ld(db: &GraphDb, _params: Option<Value>) -> Result<Value, Value> {
    let value = sparrowdb_ontology_core::export_json_ld(db)
        .map_err(|e| so_error_to_mcp(&e))?;
    let json_str = serde_json::to_string_pretty(&value)
        .map_err(|e| mcp_error(-32603, "serialization_error", json!({"detail": e.to_string()})))?;
    Ok(json!({
        "content": [{"type": "text", "text": json_str}]
    }))
}

// ── import_turtle ─────────────────────────────────────────────────────────────

pub fn tool_import_turtle(db: &GraphDb, params: Option<Value>) -> Result<Value, Value> {
    let params = params.unwrap_or(json!({}));

    let ttl = params.get("turtle")
        .and_then(|v| v.as_str())
        .ok_or_else(|| mcp_error(-32602, "turtle parameter required", json!({})))?;

    let base_iri = params.get("base_iri")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let strategy_str = params.get("strategy")
        .and_then(|v| v.as_str())
        .unwrap_or("unconstrained");

    let domain_range_strategy = if strategy_str == "first" {
        DomainRangeStrategy::FirstOnly
    } else {
        DomainRangeStrategy::Unconstrained
    };

    let opts = ImportOptions { base_iri, domain_range_strategy };

    let summary = sparrowdb_ontology_core::import_turtle(db, ttl, opts)
        .map_err(|e| so_error_to_mcp(&e))?;

    let mut result_text = format!(
        "Import complete:\n  Classes:    {}\n  Relations:  {}\n  Subclasses: {}\n  Aliases:    {}",
        summary.classes_imported,
        summary.relations_imported,
        summary.subclasses_imported,
        summary.aliases_imported,
    );

    if !summary.warnings.is_empty() {
        result_text.push_str(&format!("\nWarnings ({}):", summary.warnings.len()));
        for w in &summary.warnings {
            result_text.push_str(&format!("\n  - {w}"));
        }
    }

    Ok(json!({
        "content": [{"type": "text", "text": result_text}]
    }))
}
