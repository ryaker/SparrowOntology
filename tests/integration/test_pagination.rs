/// Pagination tests for MCP tools
///
/// Tests cursor-based and offset-based pagination for find_entities and get_ontology.
use serde_json::{json, Value};
use sparrowdb::GraphDb;
use sparrowdb_ontology_mcp::tools::handle_tool_call;

fn initialized_db() -> (tempfile::TempDir, GraphDb) {
    let dir = tempfile::tempdir().unwrap();
    let db = GraphDb::open(dir.path()).unwrap();
    sparrowdb_ontology_core::init(&db, None, false).unwrap();
    (dir, db)
}

fn call(db: &GraphDb, tool: &str, params: Value) -> Value {
    let result = handle_tool_call(db, tool, Some(params)).unwrap();
    let text = result["content"][0]["text"]
        .as_str()
        .expect("tool response must have text content");
    serde_json::from_str(text).expect("tool response must be valid JSON")
}

// ── Pagination: find_entities ─────────────────────────────────────────────────

#[test]
fn find_entities_pagination_metadata() {
    let (_dir, db) = initialized_db();

    // Create 5 Person entities
    for i in 1..=5 {
        call(
            &db,
            "create_entity",
            json!({"class_name": "Person", "properties": {"name": format!("Person{}", i)}}),
        );
    }

    // First page: limit=2, offset=0
    let page1 = call(
        &db,
        "find_entities",
        json!({"class_name": "Person", "limit": 2, "offset": 0}),
    );

    assert_eq!(
        page1["entities"].as_array().unwrap().len(),
        2,
        "should return 2 entities on first page"
    );
    assert_eq!(
        page1["pagination"]["total_count"].as_u64().unwrap(),
        5,
        "total_count should be 5"
    );
    assert_eq!(
        page1["pagination"]["offset"].as_u64().unwrap(),
        0,
        "offset should be 0"
    );
    assert_eq!(
        page1["pagination"]["limit"].as_u64().unwrap(),
        2,
        "limit should be 2"
    );
    assert!(
        page1["pagination"]["has_more"].as_bool().unwrap(),
        "has_more should be true"
    );
    assert!(
        page1["pagination"]["next_cursor"].is_string(),
        "next_cursor should be present"
    );

    // Second page: limit=2, offset=2
    let page2 = call(
        &db,
        "find_entities",
        json!({"class_name": "Person", "limit": 2, "offset": 2}),
    );

    assert_eq!(
        page2["entities"].as_array().unwrap().len(),
        2,
        "should return 2 entities on second page"
    );
    assert!(
        page2["pagination"]["has_more"].as_bool().unwrap(),
        "has_more should be true (5 total, 2 returned, 1 remaining)"
    );

    // Last page: limit=2, offset=4
    let page3 = call(
        &db,
        "find_entities",
        json!({"class_name": "Person", "limit": 2, "offset": 4}),
    );

    assert_eq!(
        page3["entities"].as_array().unwrap().len(),
        1,
        "should return 1 entity on last page"
    );
    assert!(
        !page3["pagination"]["has_more"].as_bool().unwrap(),
        "has_more should be false on last page"
    );
    assert!(
        page3["pagination"]["next_cursor"].is_null(),
        "next_cursor should be null on last page"
    );
}

#[test]
fn find_entities_cursor_pagination() {
    let (_dir, db) = initialized_db();

    // Create 6 Person entities
    for i in 1..=6 {
        call(
            &db,
            "create_entity",
            json!({"class_name": "Person", "properties": {"name": format!("Person{}", i)}}),
        );
    }

    // First page using cursor
    let page1 = call(
        &db,
        "find_entities",
        json!({"class_name": "Person", "limit": 3}),
    );

    assert_eq!(
        page1["entities"].as_array().unwrap().len(),
        3,
        "should return 3 entities on first page"
    );
    let next_cursor = page1["pagination"]["next_cursor"]
        .as_str()
        .expect("next_cursor should be a string");

    // Second page using cursor
    let page2 = call(
        &db,
        "find_entities",
        json!({"class_name": "Person", "limit": 3, "cursor": next_cursor}),
    );

    assert_eq!(
        page2["entities"].as_array().unwrap().len(),
        3,
        "should return 3 entities on second page"
    );
    assert!(
        !page2["pagination"]["has_more"].as_bool().unwrap(),
        "has_more should be false (all 6 consumed)"
    );

    // Verify no overlap — node_ids should be different
    let page1_ids: Vec<String> = page1["entities"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["node_id"].as_str().unwrap_or("").to_string())
        .collect();

    let page2_ids: Vec<String> = page2["entities"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["node_id"].as_str().unwrap_or("").to_string())
        .collect();

    for id in &page1_ids {
        assert!(
            !page2_ids.contains(id),
            "node_ids should not overlap between pages"
        );
    }
}

#[test]
fn find_entities_default_pagination() {
    let (_dir, db) = initialized_db();

    // Create 25 entities (more than default limit of 20)
    for i in 1..=25 {
        call(
            &db,
            "create_entity",
            json!({"class_name": "Person", "properties": {"name": format!("Person{}", i)}}),
        );
    }

    // Call without explicit limit/offset — should use defaults
    let result = call(&db, "find_entities", json!({"class_name": "Person"}));

    assert_eq!(
        result["entities"].as_array().unwrap().len(),
        20,
        "should return 20 entities (default limit)"
    );
    assert_eq!(
        result["pagination"]["total_count"].as_u64().unwrap(),
        25,
        "total_count should be 25"
    );
    assert!(
        result["pagination"]["has_more"].as_bool().unwrap(),
        "has_more should be true"
    );
}

// ── Pagination: get_ontology ──────────────────────────────────────────────────

#[test]
fn get_ontology_pagination_metadata() {
    let (_dir, db) = initialized_db();

    // Define 5 classes
    for i in 1..=5 {
        call(&db, "define_class", json!({"name": format!("Class{}", i)}));
    }

    // Fetch ontology with pagination: limit=2
    let ontology = call(
        &db,
        "get_ontology",
        json!({"class_limit": 2, "class_offset": 0}),
    );

    // Check classes section
    let classes = &ontology["classes"];
    assert!(
        classes["data"].is_array(),
        "classes.data should be an array"
    );
    assert!(
        classes["pagination"].is_object(),
        "classes.pagination should be an object"
    );

    // Should include built-in Thing class + 5 defined classes = 6 total
    let total = classes["pagination"]["total_count"].as_u64().unwrap();
    assert!(total >= 5, "should have at least 5 user-defined classes");

    let class_count = classes["data"].as_array().unwrap().len();
    assert_eq!(class_count, 2, "should return 2 classes (as per limit)");

    assert!(
        classes["pagination"]["has_more"].as_bool().unwrap(),
        "has_more should be true"
    );
}

#[test]
fn get_ontology_separate_pagination() {
    let (_dir, db) = initialized_db();

    // Define 3 classes and 3 relations
    for i in 1..=3 {
        call(&db, "define_class", json!({"name": format!("Class{}", i)}));
        call(
            &db,
            "define_relation",
            json!({
                "name": format!("Relation{}", i),
                "domain": format!("Class{}", i),
                "range": "Person"
            }),
        );
    }

    // Fetch with different limits for different sections
    let ontology = call(
        &db,
        "get_ontology",
        json!({
            "class_limit": 2,
            "relation_limit": 1,
            "property_limit": 10,
            "alias_limit": 10,
        }),
    );

    // Classes should be paginated with limit=2
    let class_data = ontology["classes"]["data"].as_array().unwrap();
    assert_eq!(class_data.len(), 2, "should return 2 classes (limit=2)");

    // Relations should be paginated with limit=1
    let relation_data = ontology["relations"]["data"].as_array().unwrap();
    assert_eq!(relation_data.len(), 1, "should return 1 relation (limit=1)");

    // Each section should have its own pagination metadata
    assert!(
        ontology["classes"]["pagination"]["total_count"].is_u64(),
        "classes should have pagination.total_count"
    );
    assert!(
        ontology["relations"]["pagination"]["has_more"].is_boolean(),
        "relations should have pagination.has_more"
    );
    assert!(
        ontology["properties"]["pagination"]["offset"].is_u64(),
        "properties should have pagination.offset"
    );
}

#[test]
fn get_ontology_default_limits() {
    let (_dir, db) = initialized_db();

    // Fetch without specifying limits — should use defaults (50)
    let ontology = call(&db, "get_ontology", json!({}));

    // Check that data is present even with defaults
    assert!(
        ontology["classes"]["data"].is_array(),
        "classes.data should be present"
    );
    assert!(
        ontology["relations"]["data"].is_array(),
        "relations.data should be present"
    );
    assert!(
        ontology["properties"]["data"].is_array(),
        "properties.data should be present"
    );
    assert!(
        ontology["aliases"]["data"].is_array(),
        "aliases.data should be present"
    );

    // All should have pagination info
    for section in &["classes", "relations", "properties", "aliases"] {
        assert!(
            ontology[section]["pagination"]["total_count"].is_u64(),
            "{} should have total_count",
            section
        );
        assert!(
            ontology[section]["pagination"]["has_more"].is_boolean(),
            "{} should have has_more",
            section
        );
    }
}
