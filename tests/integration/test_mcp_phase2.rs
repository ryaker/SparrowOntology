/// Phase 2 MCP integration tests — 22 acceptance criteria
///
/// All tests call `sparrowdb_ontology_mcp::tools::handle_tool_call` in-process.
/// No binary is launched.

use serde_json::{json, Value};
use sparrowdb::GraphDb;
use sparrowdb_ontology_mcp::tools::handle_tool_call;

// ── Helpers ───────────────────────────────────────────────────────────────────

fn empty_db() -> (tempfile::TempDir, GraphDb) {
    let dir = tempfile::tempdir().unwrap();
    let db = GraphDb::open(dir.path()).unwrap();
    (dir, db)
}

fn initialized_db() -> (tempfile::TempDir, GraphDb) {
    let dir = tempfile::tempdir().unwrap();
    let db = GraphDb::open(dir.path()).unwrap();
    sparrowdb_ontology_core::init(&db, None, false).unwrap();
    (dir, db)
}

/// Call a tool and expect Ok, returning parsed inner JSON (the `text` field).
fn call(db: &GraphDb, tool: &str, params: Value) -> Value {
    let result = handle_tool_call(db, tool, Some(params)).unwrap();
    // MCP wraps result: {"content": [{"type": "text", "text": "<json>"}]}
    let text = result["content"][0]["text"].as_str().unwrap_or("{}");
    serde_json::from_str(text).unwrap_or(json!({}))
}

/// Call a tool and expect Err, returning the error Value.
fn call_err(db: &GraphDb, tool: &str, params: Value) -> Value {
    handle_tool_call(db, tool, Some(params)).unwrap_err()
}

// ── AC01: start_here on uninitialized DB ──────────────────────────────────────

#[test]
fn ac01_start_here_uninitialized() {
    let (_dir, db) = empty_db();
    let result = call(&db, "start_here", json!({}));
    assert_eq!(
        result["status"].as_str().unwrap_or(""),
        "uninitialized",
        "status should be 'uninitialized', got: {result}"
    );
    // next_steps should mention world_model / init
    let next_steps = result["next_steps"].as_array().unwrap();
    assert!(
        !next_steps.is_empty(),
        "next_steps should be non-empty for uninitialized state"
    );
    let steps_text = next_steps
        .iter()
        .filter_map(|v| v.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        steps_text.to_lowercase().contains("init")
            || steps_text.to_lowercase().contains("world")
            || steps_text.to_lowercase().contains("ontology"),
        "next_steps should mention init/world_model: {steps_text}"
    );
}

// ── AC02: start_here on initialized DB ───────────────────────────────────────

#[test]
fn ac02_start_here_initialized() {
    let (_dir, db) = initialized_db();
    let result = call(&db, "start_here", json!({}));
    assert_eq!(
        result["status"].as_str().unwrap_or(""),
        "initialized",
        "status should be 'initialized', got: {result}"
    );
    let class_count = result["ontology"]["class_count"]
        .as_i64()
        .expect("ontology.class_count should be an integer");
    assert_eq!(
        class_count, 10,
        "ontology.class_count should be 10, got: {class_count}"
    );
}

// ── AC03: get_ontology returns all classes and relations ──────────────────────

#[test]
fn ac03_get_ontology_returns_all() {
    let (_dir, db) = initialized_db();
    let result = call(&db, "get_ontology", json!({}));
    let classes = result["classes"].as_array().expect("classes should be an array");
    assert_eq!(
        classes.len(),
        10,
        "classes array should have 10 items, got: {}",
        classes.len()
    );
    let relations = result["relations"].as_array().expect("relations should be an array");
    assert_eq!(
        relations.len(),
        19,
        "relations array should have 19 items, got: {}",
        relations.len()
    );
}

// ── AC04: define_class creates a node ────────────────────────────────────────

#[test]
fn ac04_define_class_creates_node() {
    let (_dir, db) = initialized_db();
    let result = call(&db, "define_class", json!({"name": "Employee"}));
    // Response: {"created": {symbol_id, name, ...}}
    let created = &result["created"];
    assert_eq!(
        created["name"].as_str().unwrap_or(""),
        "Employee",
        "created.name should be 'Employee', got: {result}"
    );
    assert!(
        !created["symbol_id"].as_str().unwrap_or("").is_empty(),
        "created.symbol_id should be present"
    );
}

// ── AC05: define_relation validates domain and range ─────────────────────────

#[test]
fn ac05_define_relation_validates_domain_range() {
    let (_dir, db) = initialized_db();
    let result = call(
        &db,
        "define_relation",
        json!({"name": "MANAGES", "domain": "Person", "range": "Project"}),
    );
    let created = &result["created"];
    assert_eq!(
        created["name"].as_str().unwrap_or(""),
        "MANAGES",
        "created.name should be 'MANAGES', got: {result}"
    );
    assert_eq!(
        created["domain"].as_str().unwrap_or(""),
        "Person",
        "created.domain should be 'Person'"
    );
    assert_eq!(
        created["range"].as_str().unwrap_or(""),
        "Project",
        "created.range should be 'Project'"
    );
}

// ── AC06: add_alias then resolve_name ────────────────────────────────────────

#[test]
fn ac06_add_alias_then_resolve_name() {
    let (_dir, db) = initialized_db();

    // Add alias: EMPLOYED_BY → WORKS_FOR (relation alias)
    let alias_result = call(
        &db,
        "add_alias",
        json!({"alias_name": "EMPLOYED_BY", "target": "WORKS_FOR", "kind": "relation"}),
    );
    assert_eq!(
        alias_result["success"].as_bool().unwrap_or(false),
        true,
        "add_alias should succeed, got: {alias_result}"
    );

    // Resolve the alias
    let resolve_result = call(
        &db,
        "resolve_name",
        json!({"name": "EMPLOYED_BY", "kind": "relation"}),
    );
    assert_eq!(
        resolve_result["canonical_name"].as_str().unwrap_or(""),
        "WORKS_FOR",
        "canonical_name should be 'WORKS_FOR', got: {resolve_result}"
    );
    assert_eq!(
        resolve_result["was_alias"].as_bool().unwrap_or(false),
        true,
        "was_alias should be true"
    );
}

// ── AC07: create_entity for Person ───────────────────────────────────────────

#[test]
fn ac07_create_entity_person() {
    let (_dir, db) = initialized_db();
    let result = call(
        &db,
        "create_entity",
        json!({"class_name": "Person", "properties": {"name": "Alice"}}),
    );
    assert_eq!(
        result["created"].as_bool().unwrap_or(false),
        true,
        "created should be true, got: {result}"
    );
    assert!(
        !result["node_id"].as_str().unwrap_or("").is_empty(),
        "node_id should be present, got: {result}"
    );
}

// ── AC08: create_entity via alias ────────────────────────────────────────────

#[test]
fn ac08_create_entity_via_alias() {
    let (_dir, db) = initialized_db();
    // Register "Human" as an alias for "Person" first
    call(
        &db,
        "add_alias",
        json!({"alias_name": "Human", "target": "Person", "kind": "class"}),
    );
    let result = call(
        &db,
        "create_entity",
        json!({"class_name": "Human", "properties": {"name": "Bob"}}),
    );
    assert_eq!(
        result["created"].as_bool().unwrap_or(false),
        true,
        "created should be true for alias 'Human', got: {result}"
    );
    assert_eq!(
        result["canonical_label"].as_str().unwrap_or(""),
        "Person",
        "canonical_label should be 'Person' when created via alias 'Human', got: {result}"
    );
}

// ── AC09: create_entity preserve_source_terms ────────────────────────────────

#[test]
fn ac09_create_entity_preserve_source_terms() {
    let (_dir, db) = initialized_db();
    // Register "Human" as an alias for "Person" first
    call(
        &db,
        "add_alias",
        json!({"alias_name": "Human", "target": "Person", "kind": "class"}),
    );
    let result = call(
        &db,
        "create_entity",
        json!({
            "class_name": "Human",
            "properties": {"name": "Bob"},
            "preserve_source_terms": true
        }),
    );
    assert_eq!(
        result["created"].as_bool().unwrap_or(false),
        true,
        "created should be true, got: {result}"
    );
    // source_label should be "Human" (the alias used)
    let source_label = result["source_label"].as_str().unwrap_or("");
    assert_eq!(
        source_label, "Human",
        "source_label should be 'Human' when preserve_source_terms=true, got: {result}"
    );
}

// ── AC10: create_entity unknown class returns error ──────────────────────────

#[test]
fn ac10_create_entity_unknown_class_error() {
    let (_dir, db) = initialized_db();
    let err = call_err(
        &db,
        "create_entity",
        json!({"class_name": "Stakeholder", "properties": {}}),
    );
    let data = &err["data"];
    assert_eq!(
        data["error_kind"].as_str().unwrap_or(""),
        "UnknownSymbol",
        "error_kind should be 'UnknownSymbol', got: {err}"
    );
    assert!(
        !data["suggestion"].as_str().unwrap_or("").is_empty(),
        "suggestion should be present for UnknownSymbol, got: {err}"
    );
}

// ── AC11: create_relationship valid ──────────────────────────────────────────

#[test]
fn ac11_create_relationship_valid() {
    let (_dir, db) = initialized_db();

    // Create Person node
    let alice = call(
        &db,
        "create_entity",
        json!({"class_name": "Person", "properties": {"name": "Alice"}}),
    );
    let alice_id = alice["node_id"].as_str().expect("node_id should be present");

    // Create Organization node
    let acme = call(
        &db,
        "create_entity",
        json!({"class_name": "Organization", "properties": {"name": "Acme"}}),
    );
    let acme_id = acme["node_id"].as_str().expect("node_id should be present");

    // Create relationship
    let result = call(
        &db,
        "create_relationship",
        json!({"from_id": alice_id, "relation_name": "WORKS_FOR", "to_id": acme_id}),
    );
    assert_eq!(
        result["created"].as_bool().unwrap_or(false),
        true,
        "created should be true, got: {result}"
    );
}

// ── AC12: create_relationship via alias ──────────────────────────────────────

#[test]
fn ac12_create_relationship_via_alias() {
    let (_dir, db) = initialized_db();

    // Register EMPLOYED_BY as alias for WORKS_FOR
    call(
        &db,
        "add_alias",
        json!({"alias_name": "EMPLOYED_BY", "target": "WORKS_FOR", "kind": "relation"}),
    );

    // Create nodes
    let alice = call(
        &db,
        "create_entity",
        json!({"class_name": "Person", "properties": {"name": "Alice"}}),
    );
    let alice_id = alice["node_id"].as_str().expect("node_id should be present");

    let acme = call(
        &db,
        "create_entity",
        json!({"class_name": "Organization", "properties": {"name": "Acme"}}),
    );
    let acme_id = acme["node_id"].as_str().expect("node_id should be present");

    // Use the alias as rel_type
    let result = call(
        &db,
        "create_relationship",
        json!({"from_id": alice_id, "relation_name": "EMPLOYED_BY", "to_id": acme_id}),
    );
    assert_eq!(
        result["created"].as_bool().unwrap_or(false),
        true,
        "created should be true via alias 'EMPLOYED_BY', got: {result}"
    );
}

// ── AC13: create_relationship invalid domain ─────────────────────────────────

#[test]
fn ac13_create_relationship_invalid_domain() {
    let (_dir, db) = initialized_db();

    // Create nodes: org → person (swapped — WORKS_FOR expects Person→Organization)
    let acme = call(
        &db,
        "create_entity",
        json!({"class_name": "Organization", "properties": {"name": "Acme"}}),
    );
    let acme_id = acme["node_id"].as_str().expect("node_id should be present");

    let alice = call(
        &db,
        "create_entity",
        json!({"class_name": "Person", "properties": {"name": "Alice"}}),
    );
    let alice_id = alice["node_id"].as_str().expect("node_id should be present");

    // Swap from/to — Organization→Person for WORKS_FOR is wrong domain
    let err = call_err(
        &db,
        "create_relationship",
        json!({"from_id": acme_id, "relation_name": "WORKS_FOR", "to_id": alice_id}),
    );
    let data = &err["data"];
    assert_eq!(
        data["error_kind"].as_str().unwrap_or(""),
        "DomainViolation",
        "error_kind should be 'DomainViolation', got: {err}"
    );
    assert!(
        !data["suggestion"].as_str().unwrap_or("").is_empty(),
        "suggestion should be present for DomainViolation, got: {err}"
    );
}

// ── AC14: update_entity validates ────────────────────────────────────────────

#[test]
fn ac14_update_entity_validates() {
    let (_dir, db) = initialized_db();

    // Create a Person node
    let result = call(
        &db,
        "create_entity",
        json!({"class_name": "Person", "properties": {"name": "Alice"}}),
    );
    let node_id = result["node_id"].as_str().expect("node_id should be present");

    // Valid update: name is a string property
    let update_result = call(
        &db,
        "update_entity",
        json!({"node_id": node_id, "properties": {"name": "Alicia"}}),
    );
    assert_eq!(
        update_result["updated"].as_bool().unwrap_or(false),
        true,
        "updated should be true for valid property, got: {update_result}"
    );

    // Update with wrong type: name should be String, not integer → TypeMismatch
    let err = call_err(
        &db,
        "update_entity",
        json!({"node_id": node_id, "properties": {"name": 42}}),
    );
    let data = &err["data"];
    assert_eq!(
        data["error_kind"].as_str().unwrap_or(""),
        "TypeMismatch",
        "error_kind should be 'TypeMismatch' for wrong property type, got: {err}"
    );
}

// ── AC15: find_entities by label ─────────────────────────────────────────────

#[test]
fn ac15_find_entities_by_label() {
    let (_dir, db) = initialized_db();

    // Create 2 Person nodes
    call(
        &db,
        "create_entity",
        json!({"class_name": "Person", "properties": {"name": "Alice"}}),
    );
    call(
        &db,
        "create_entity",
        json!({"class_name": "Person", "properties": {"name": "Bob"}}),
    );

    let result = call(&db, "find_entities", json!({"class_name": "Person"}));
    let entities = result["entities"].as_array().expect("entities should be an array");
    assert_eq!(
        entities.len(),
        2,
        "should find 2 Person entities, got: {}",
        entities.len()
    );
}

// ── AC16: find_entities via alias ────────────────────────────────────────────

#[test]
fn ac16_find_entities_via_alias() {
    let (_dir, db) = initialized_db();

    // Register "Human" as alias for "Person"
    call(
        &db,
        "add_alias",
        json!({"alias_name": "Human", "target": "Person", "kind": "class"}),
    );

    // Create a Person node
    call(
        &db,
        "create_entity",
        json!({"class_name": "Person", "properties": {"name": "Alice"}}),
    );

    // Search via alias "Human"
    let result = call(&db, "find_entities", json!({"class_name": "Human"}));
    let entities = result["entities"].as_array().expect("entities should be an array");
    assert!(
        !entities.is_empty(),
        "should return Person entities when searching by alias 'Human', got: {result}"
    );
}

// ── AC17: find_entities include_subclasses ───────────────────────────────────

#[test]
fn ac17_find_entities_include_subclasses() {
    let (_dir, db) = initialized_db();

    // Define Employee as subclass of Person
    call(&db, "define_class", json!({"name": "Employee"}));
    call(
        &db,
        "define_subclass",
        json!({"child": "Employee", "parent": "Person"}),
    );

    // Create an Employee node — Employee has no registered properties so use empty map
    // Use WriteTx directly to bypass validation for the subclass entity
    {
        use std::collections::HashMap;
        use sparrowdb_storage::node_store::Value as StoreValue;
        let mut tx = db.begin_write().unwrap();
        let mut m: HashMap<String, StoreValue> = HashMap::new();
        m.insert("name".to_string(), StoreValue::Bytes(b"Charlie".to_vec()));
        tx.merge_node("Employee", m).unwrap();
        tx.commit().unwrap();
    }

    // Create a Person node via the tool
    call(
        &db,
        "create_entity",
        json!({"class_name": "Person", "properties": {"name": "Alice"}}),
    );

    // Find Person with include_subclasses=true — should include Employee instances
    let result = call(
        &db,
        "find_entities",
        json!({"class_name": "Person", "include_subclasses": true}),
    );
    let entities = result["entities"].as_array().expect("entities should be an array");
    assert!(
        entities.len() >= 2,
        "should find at least 2 entities (Person + Employee subclass), got: {}",
        entities.len()
    );
    // Verify Employee is included
    let labels: Vec<&str> = entities
        .iter()
        .filter_map(|e| e["label"].as_str())
        .collect();
    assert!(
        labels.contains(&"Employee"),
        "should include Employee entities when include_subclasses=true, got labels: {labels:?}"
    );
}

// ── AC18: find_entities with where filter ────────────────────────────────────

#[test]
fn ac18_find_entities_with_where_filter() {
    let (_dir, db) = initialized_db();

    call(
        &db,
        "create_entity",
        json!({"class_name": "Person", "properties": {"name": "Alice"}}),
    );
    call(
        &db,
        "create_entity",
        json!({"class_name": "Person", "properties": {"name": "Bob"}}),
    );

    let result = call(
        &db,
        "find_entities",
        json!({"class_name": "Person", "where": {"name": "Alice"}}),
    );
    let entities = result["entities"].as_array().expect("entities should be an array");
    assert_eq!(
        entities.len(),
        1,
        "where filter name=Alice should return exactly 1 entity (not Bob), got: {}. Full result: {result}",
        entities.len()
    );
}

// ── AC19: explain_symbol class ───────────────────────────────────────────────

#[test]
fn ac19_explain_symbol_class() {
    let (_dir, db) = initialized_db();
    // Register "Human" as alias for "Person" so explain_symbol shows it
    call(
        &db,
        "add_alias",
        json!({"alias_name": "Human", "target": "Person", "kind": "class"}),
    );
    let result = call(
        &db,
        "explain_symbol",
        json!({"name": "Person", "kind": "class"}),
    );
    assert_eq!(
        result["kind"].as_str().unwrap_or(""),
        "class",
        "kind should be 'class', got: {result}"
    );
    // aliases should be present (is an array)
    let aliases = result["aliases"]
        .as_array()
        .expect("aliases should be an array");
    // Person now has alias "Human" registered
    assert!(
        aliases.iter().any(|a| a.as_str() == Some("Human")),
        "Person aliases should contain 'Human', got: {aliases:?}"
    );
    // properties should be present
    let properties = result["properties"]
        .as_array()
        .expect("properties should be an array");
    assert!(
        !properties.is_empty(),
        "Person should have properties defined"
    );
    // valid_relations_as_source should contain WORKS_FOR
    let valid_rels = result["valid_relations_as_source"]
        .as_array()
        .expect("valid_relations_as_source should be an array");
    assert!(
        valid_rels.iter().any(|r| r.as_str() == Some("WORKS_FOR")),
        "valid_relations_as_source should contain 'WORKS_FOR', got: {valid_rels:?}"
    );
}

// ── AC20: explain_symbol relation ────────────────────────────────────────────

#[test]
fn ac20_explain_symbol_relation() {
    let (_dir, db) = initialized_db();
    let result = call(
        &db,
        "explain_symbol",
        json!({"name": "WORKS_FOR", "kind": "relation"}),
    );
    assert_eq!(
        result["kind"].as_str().unwrap_or(""),
        "relation",
        "kind should be 'relation', got: {result}"
    );
    assert_eq!(
        result["domain"].as_str().unwrap_or(""),
        "Person",
        "domain should be 'Person', got: {result}"
    );
    assert_eq!(
        result["range"].as_str().unwrap_or(""),
        "Organization",
        "range should be 'Organization', got: {result}"
    );
    let valid_source = result["valid_source_classes"]
        .as_array()
        .expect("valid_source_classes should be an array");
    assert!(
        valid_source.iter().any(|c| c.as_str() == Some("Person")),
        "valid_source_classes should contain 'Person', got: {valid_source:?}"
    );
}

// ── AC21: validate clean graph ───────────────────────────────────────────────

#[test]
fn ac21_validate_clean_graph() {
    let (_dir, db) = initialized_db();
    let result = call(&db, "validate", json!({"scope": "full_graph"}));
    assert_eq!(
        result["valid"].as_bool().unwrap_or(false),
        true,
        "valid should be true for clean initialized graph, got: {result}"
    );
    let violations = result["violations"]
        .as_array()
        .expect("violations should be an array");
    assert!(
        violations.is_empty(),
        "violations should be empty for clean graph, got: {violations:?}"
    );
}

// ── AC22: validate reports violations for unknown label ──────────────────────

#[test]
fn ac22_validate_reports_violations() {
    let (_dir, db) = initialized_db();

    // Directly insert a node with an unknown label using low-level WriteTx
    // (bypassing the ontology validation layer)
    {
        use std::collections::HashMap;
        use sparrowdb_storage::node_store::Value as StoreValue;
        let mut tx = db.begin_write().unwrap();
        tx.merge_node(
            "UnknownType",
            {
                let mut m: HashMap<String, StoreValue> = HashMap::new();
                m.insert(
                    "name".to_string(),
                    StoreValue::Bytes(b"rogue".to_vec()),
                );
                m
            },
        )
        .unwrap();
        tx.commit().unwrap();
    }

    let result = call(&db, "validate", json!({"scope": "full_graph"}));
    assert_eq!(
        result["valid"].as_bool().unwrap_or(true),
        false,
        "valid should be false when unknown-label nodes exist, got: {result}"
    );
    let violations = result["violations"]
        .as_array()
        .expect("violations should be an array");
    assert!(
        !violations.is_empty(),
        "violations should be non-empty when unknown-label node exists, got: {result}"
    );
}

// ── AC23: validate with real entities+relationships returns no false positives ─
// Regression test: CALL db.schema() returns both node labels and relationship
// type names. Without filtering, relation names like "WORKS_FOR" were flagged
// as UnknownClass violations on graphs that had edges.

#[test]
fn ac23_validate_no_false_positives_with_relationships() {
    let (_dir, db) = initialized_db();

    // Create two entities
    let alice = call(
        &db,
        "create_entity",
        json!({"class_name": "Person", "properties": {"name": "Alice"}}),
    );
    let alice_id = alice["node_id"].as_str().expect("node_id").to_string();

    let acme = call(
        &db,
        "create_entity",
        json!({"class_name": "Organization", "properties": {"name": "Acme"}}),
    );
    let acme_id = acme["node_id"].as_str().expect("node_id").to_string();

    // Create a relationship between them
    let rel = call(
        &db,
        "create_relationship",
        json!({"from_id": alice_id, "to_id": acme_id, "relation_name": "WORKS_FOR"}),
    );
    assert!(rel["created"].as_bool().unwrap_or(false), "relationship should be created");

    // Validate — must not produce false-positive violations for "WORKS_FOR" label
    let result = call(&db, "validate", json!({"scope": "full_graph"}));
    assert_eq!(
        result["valid"].as_bool().unwrap_or(false),
        true,
        "validate should be clean after creating typed entities + relationship, got: {result}"
    );
    let violations = result["violations"].as_array().expect("violations array");
    assert!(
        violations.is_empty(),
        "relation type names must not appear as UnknownClass violations, got: {violations:?}"
    );
}
