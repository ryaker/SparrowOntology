/// Phase 3 CLI integration tests — 11 acceptance criteria
///
/// All tests call `handle_tool_call` in-process (same pattern as Phase 2),
/// but validate CLI-layer behaviors: init result counts, alias resolution,
/// error suggestion fields, etc.
use serde_json::{json, Value};
use sparrowdb::GraphDb;
use sparrowdb_ontology_core::{init, SoError};
use sparrowdb_ontology_mcp::tools::handle_tool_call;

// ── Helpers ───────────────────────────────────────────────────────────────────

fn initialized_db() -> (tempfile::TempDir, GraphDb) {
    let dir = tempfile::tempdir().unwrap();
    let db = GraphDb::open(dir.path()).unwrap();
    init(&db, None, false).unwrap();
    (dir, db)
}

/// Call a tool and expect Ok, returning the inner parsed JSON (from content[0].text).
fn call(db: &GraphDb, tool: &str, params: Value) -> Value {
    let result = handle_tool_call(db, tool, Some(params)).unwrap();
    let text = result["content"][0]["text"].as_str().unwrap_or("{}");
    serde_json::from_str(text).unwrap_or(json!({}))
}

/// Call a tool and expect Err, returning the error Value.
fn call_err(db: &GraphDb, tool: &str, params: Value) -> Value {
    handle_tool_call(db, tool, Some(params)).unwrap_err()
}

// ── AC01: cli_init_creates_world_model ────────────────────────────────────────

#[test]
fn cli_init_creates_world_model() {
    let dir = tempfile::tempdir().unwrap();
    let db = GraphDb::open(dir.path()).unwrap();

    let result = init(&db, None, false).expect("init should succeed on fresh DB");

    assert_eq!(
        result.classes_created, 10,
        "should create 10 classes, got: {}",
        result.classes_created
    );
    assert_eq!(
        result.relations_created, 19,
        "should create 19 relations, got: {}",
        result.relations_created
    );
    assert_eq!(
        result.properties_created, 22,
        "should create 22 properties, got: {}",
        result.properties_created
    );
}

// ── AC02: cli_init_fails_second_run ──────────────────────────────────────────

#[test]
fn cli_init_fails_second_run() {
    let dir = tempfile::tempdir().unwrap();
    let db = GraphDb::open(dir.path()).unwrap();

    init(&db, None, false).expect("first init should succeed");

    let result = init(&db, None, false);
    match result {
        Err(SoError::AlreadyInitialized) => {
            // Correct — second init without force returns AlreadyInitialized
        }
        Ok(_) => panic!("second init without force should have failed with AlreadyInitialized"),
        Err(e) => panic!("second init returned unexpected error: {e}"),
    }
}

// ── AC03: cli_init_force_resets ───────────────────────────────────────────────

#[test]
fn cli_init_force_resets() {
    let dir = tempfile::tempdir().unwrap();
    let db = GraphDb::open(dir.path()).unwrap();

    init(&db, None, false).expect("first init should succeed");

    // force=true must succeed even though already initialized
    let result = init(&db, None, true).expect("init with force=true should succeed");

    assert_eq!(
        result.classes_created, 10,
        "force re-init should produce 10 classes, got: {}",
        result.classes_created
    );
    assert_eq!(
        result.relations_created, 19,
        "force re-init should produce 19 relations, got: {}",
        result.relations_created
    );
    assert_eq!(
        result.properties_created, 22,
        "force re-init should produce 22 properties, got: {}",
        result.properties_created
    );
}

// ── AC04: cli_show_summary ────────────────────────────────────────────────────

#[test]
fn cli_show_summary() {
    let (_dir, db) = initialized_db();
    let result = call(&db, "get_ontology", json!({}));

    let classes = result["classes"]["data"]
        .as_array()
        .expect("get_ontology should return 'classes.data' array");
    assert_eq!(
        classes.len(),
        10,
        "should have 10 classes in ontology, got: {}",
        classes.len()
    );
}

// ── AC05: cli_show_full ───────────────────────────────────────────────────────

#[test]
fn cli_show_full() {
    let (_dir, db) = initialized_db();
    let result = call(&db, "get_ontology", json!({}));

    let classes = result["classes"]["data"]
        .as_array()
        .expect("get_ontology should return 'classes.data' array");

    // In full mode, each class should include a "properties" array
    // At least one class (Person) should have non-empty properties
    let has_properties = classes.iter().any(|c| {
        c["properties"]
            .as_array()
            .map(|p| !p.is_empty())
            .unwrap_or(false)
    });
    assert!(
        has_properties,
        "At least one class should have properties defined in full ontology view, classes: {:?}",
        classes
            .iter()
            .map(|c| c["name"].as_str())
            .collect::<Vec<_>>()
    );
}

// ── AC06: cli_validate_clean ──────────────────────────────────────────────────

#[test]
fn cli_validate_clean() {
    let (_dir, db) = initialized_db();
    let result = call(&db, "validate", json!({"scope": "full_graph"}));

    assert_eq!(
        result["valid"].as_bool().unwrap_or(false),
        true,
        "freshly initialized DB should be valid, got: {result}"
    );
    let violations = result["violations"]
        .as_array()
        .expect("violations should be an array");
    assert!(
        violations.is_empty(),
        "no violations expected for clean initialized DB, got: {violations:?}"
    );
}

// ── AC07: cli_resolve_alias ───────────────────────────────────────────────────

#[test]
fn cli_resolve_alias() {
    let (_dir, db) = initialized_db();

    // Add alias EMPLOYED_BY → WORKS_FOR
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
        "was_alias should be true when resolving an alias"
    );
}

// ── AC08: cli_add_alias ───────────────────────────────────────────────────────

#[test]
fn cli_add_alias() {
    let (_dir, db) = initialized_db();

    // Add alias TEST → Person (class)
    let alias_result = call(
        &db,
        "add_alias",
        json!({"alias_name": "TEST", "target": "Person", "kind": "class"}),
    );
    assert_eq!(
        alias_result["success"].as_bool().unwrap_or(false),
        true,
        "add_alias TEST → Person should succeed, got: {alias_result}"
    );

    // Resolve the alias
    let resolve_result = call(
        &db,
        "resolve_name",
        json!({"name": "TEST", "kind": "class"}),
    );
    assert_eq!(
        resolve_result["canonical_name"].as_str().unwrap_or(""),
        "Person",
        "canonical_name should be 'Person', got: {resolve_result}"
    );
    assert_eq!(
        resolve_result["was_alias"].as_bool().unwrap_or(false),
        true,
        "was_alias should be true for 'TEST', got: {resolve_result}"
    );
}

// ── AC09: cli_create_entity_person ───────────────────────────────────────────

#[test]
fn cli_create_entity_person() {
    let (_dir, db) = initialized_db();

    let result = call(
        &db,
        "create_entity",
        json!({"label": "Person", "properties": {"name": "Alice"}}),
    );
    assert_eq!(
        result["created"].as_bool().unwrap_or(false),
        true,
        "created should be true, got: {result}"
    );
    assert!(
        !result["node_id"].as_str().unwrap_or("").is_empty(),
        "node_id should be present and non-empty, got: {result}"
    );
}

// ── AC10: cli_all_commands_exit_ok ────────────────────────────────────────────

#[test]
fn cli_all_commands_exit_ok() {
    let (_dir, db) = initialized_db();

    // start_here
    let r = handle_tool_call(&db, "start_here", Some(json!({})));
    assert!(r.is_ok(), "start_here should return Ok, got: {:?}", r.err());

    // get_ontology
    let r = handle_tool_call(&db, "get_ontology", Some(json!({})));
    assert!(
        r.is_ok(),
        "get_ontology should return Ok, got: {:?}",
        r.err()
    );

    // define_class
    let r = handle_tool_call(&db, "define_class", Some(json!({"name": "Employee"})));
    assert!(
        r.is_ok(),
        "define_class should return Ok, got: {:?}",
        r.err()
    );

    // define_relation
    let r = handle_tool_call(
        &db,
        "define_relation",
        Some(json!({"name": "MANAGES", "domain": "Person", "range": "Project"})),
    );
    assert!(
        r.is_ok(),
        "define_relation should return Ok, got: {:?}",
        r.err()
    );

    // add_alias
    let r = handle_tool_call(
        &db,
        "add_alias",
        Some(json!({"alias_name": "Worker", "target": "Person", "kind": "class"})),
    );
    assert!(r.is_ok(), "add_alias should return Ok, got: {:?}", r.err());

    // resolve_name
    let r = handle_tool_call(
        &db,
        "resolve_name",
        Some(json!({"name": "Person", "kind": "class"})),
    );
    assert!(
        r.is_ok(),
        "resolve_name should return Ok, got: {:?}",
        r.err()
    );

    // define_subclass
    let r = handle_tool_call(
        &db,
        "define_subclass",
        Some(json!({"child": "Employee", "parent": "Person"})),
    );
    assert!(
        r.is_ok(),
        "define_subclass should return Ok, got: {:?}",
        r.err()
    );

    // create_entity
    let r = handle_tool_call(
        &db,
        "create_entity",
        Some(json!({"label": "Person", "properties": {"name": "Bob"}})),
    );
    assert!(
        r.is_ok(),
        "create_entity should return Ok, got: {:?}",
        r.err()
    );

    // validate
    let r = handle_tool_call(&db, "validate", Some(json!({"scope": "full_graph"})));
    assert!(r.is_ok(), "validate should return Ok, got: {:?}", r.err());

    // explain_symbol (class)
    let r = handle_tool_call(
        &db,
        "explain_symbol",
        Some(json!({"name": "Person", "kind": "class"})),
    );
    assert!(
        r.is_ok(),
        "explain_symbol should return Ok, got: {:?}",
        r.err()
    );

    // explain_symbol (relation)
    let r = handle_tool_call(
        &db,
        "explain_symbol",
        Some(json!({"name": "WORKS_FOR", "kind": "relation"})),
    );
    assert!(
        r.is_ok(),
        "explain_symbol WORKS_FOR should return Ok, got: {:?}",
        r.err()
    );
}

// ── AC11: cli_error_output_has_suggestion ────────────────────────────────────

#[test]
fn cli_error_output_has_suggestion() {
    let (_dir, db) = initialized_db();

    // create_entity with unknown class "Stakeholder" should return an error with a suggestion
    let err = call_err(
        &db,
        "create_entity",
        json!({"label": "Stakeholder", "properties": {}}),
    );

    let data = &err["data"];
    assert_eq!(
        data["error_kind"].as_str().unwrap_or(""),
        "UnknownSymbol",
        "error_kind should be 'UnknownSymbol', got: {err}"
    );

    let suggestion = data["suggestion"].as_str().unwrap_or("");
    assert!(
        !suggestion.is_empty(),
        "suggestion field should be present and non-empty in error data, got: {err}"
    );
}
