/// SPA-269/270 integration tests — health and stats tools
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

/// Call a tool and expect Ok, returning the parsed inner JSON (the `text` field).
fn call(db: &GraphDb, tool: &str, params: Value) -> Value {
    let result = handle_tool_call(db, tool, Some(params)).unwrap();
    let text = result["content"][0]["text"].as_str().unwrap_or("{}");
    serde_json::from_str(text).unwrap_or(json!({}))
}

// ── SPA-269: health tool ──────────────────────────────────────────────────────

#[test]
fn health_on_initialized_db_returns_ok() {
    let (_dir, db) = initialized_db();
    let result = call(&db, "health", json!({}));

    assert_eq!(
        result["status"].as_str().unwrap_or(""),
        "ok",
        "health status should be 'ok', got: {result}"
    );
    assert_eq!(
        result["service"].as_str().unwrap_or(""),
        "sparrow-ontology-mcp",
        "service name should be 'sparrow-ontology-mcp', got: {result}"
    );
    assert!(
        result["db_connected"].as_bool().unwrap_or(false),
        "db_connected should be true, got: {result}"
    );
    let class_count = result["class_count"]
        .as_i64()
        .expect("class_count should be an integer");
    assert!(
        class_count > 0,
        "class_count should be > 0 after init, got: {class_count}"
    );
    let relation_count = result["relation_count"]
        .as_i64()
        .expect("relation_count should be an integer");
    assert!(
        relation_count > 0,
        "relation_count should be > 0 after init, got: {relation_count}"
    );
}

#[test]
fn health_on_empty_db_still_reports_connected() {
    let (_dir, db) = empty_db();
    let result = call(&db, "health", json!({}));

    assert_eq!(
        result["status"].as_str().unwrap_or(""),
        "ok",
        "health status should be 'ok' even on empty db, got: {result}"
    );
    assert!(
        result["db_connected"].as_bool().unwrap_or(false),
        "db_connected should be true even when ontology is not initialized, got: {result}"
    );
    assert_eq!(
        result["class_count"].as_i64().unwrap_or(-1),
        0,
        "class_count should be 0 on uninitialized db, got: {result}"
    );
}

// ── SPA-270: stats tool ───────────────────────────────────────────────────────

#[test]
fn stats_on_initialized_db_returns_schema_counts() {
    let (_dir, db) = initialized_db();
    let result = call(&db, "stats", json!({}));

    let schema = &result["schema"];
    let class_count = schema["class_count"]
        .as_i64()
        .expect("schema.class_count should be an integer");
    assert_eq!(
        class_count, 10,
        "schema.class_count should be 10 after world-model init, got: {class_count}"
    );

    let relation_count = schema["relation_count"]
        .as_i64()
        .expect("schema.relation_count should be an integer");
    assert_eq!(
        relation_count, 19,
        "schema.relation_count should be 19, got: {relation_count}"
    );

    let property_count = schema["property_count"]
        .as_i64()
        .expect("schema.property_count should be an integer");
    assert!(
        property_count > 0,
        "schema.property_count should be > 0 after init, got: {property_count}"
    );

    let unseeded = schema["unseeded_classes"]
        .as_array()
        .expect("schema.unseeded_classes should be an array");
    // After full world-model init all classes have properties
    assert!(
        unseeded.is_empty(),
        "unseeded_classes should be empty after full world-model init, got: {unseeded:?}"
    );
}

#[test]
fn stats_entities_section_is_present() {
    let (_dir, db) = initialized_db();

    // Create a Person entity so total > 0
    handle_tool_call(
        &db,
        "create_entity",
        Some(json!({"class_name": "Person", "properties": {"name": "Alice"}})),
    )
    .unwrap();

    let result = call(&db, "stats", json!({}));

    let entities = &result["entities"];
    let total = entities["total"]
        .as_i64()
        .expect("entities.total should be an integer");
    assert_eq!(
        total, 1,
        "entities.total should be 1 after creating one entity"
    );

    let by_class = entities["by_class"]
        .as_object()
        .expect("entities.by_class should be an object");
    assert!(
        by_class.contains_key("Person"),
        "entities.by_class should contain 'Person' key, got: {by_class:?}"
    );
    assert_eq!(
        by_class["Person"].as_i64().unwrap_or(0),
        1,
        "entities.by_class.Person should be 1"
    );
}

#[test]
fn stats_unseeded_classes_lists_newly_defined_class() {
    let (_dir, db) = initialized_db();

    // Define a new class without adding any properties — it should appear as unseeded
    handle_tool_call(&db, "define_class", Some(json!({"name": "Widget"}))).unwrap();

    let result = call(&db, "stats", json!({}));

    let schema = &result["schema"];
    let unseeded = schema["unseeded_classes"]
        .as_array()
        .expect("schema.unseeded_classes should be an array");
    assert!(
        unseeded.iter().any(|v| v.as_str() == Some("Widget")),
        "unseeded_classes should contain 'Widget' (no properties added), got: {unseeded:?}"
    );
}

#[test]
fn stats_on_empty_db_returns_zero_counts() {
    let (_dir, db) = empty_db();
    let result = call(&db, "stats", json!({}));

    let schema = &result["schema"];
    assert_eq!(
        schema["class_count"].as_i64().unwrap_or(-1),
        0,
        "class_count should be 0 on empty db"
    );
    assert_eq!(
        schema["relation_count"].as_i64().unwrap_or(-1),
        0,
        "relation_count should be 0 on empty db"
    );
    assert_eq!(
        schema["property_count"].as_i64().unwrap_or(-1),
        0,
        "property_count should be 0 on empty db"
    );

    let entities = &result["entities"];
    assert_eq!(
        entities["total"].as_i64().unwrap_or(-1),
        0,
        "entities.total should be 0 on empty db"
    );
}
