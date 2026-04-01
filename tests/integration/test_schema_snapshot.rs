use sparrowdb::GraphDb;
use sparrowdb_ontology_core::{
    add_alias, add_property, define_subclass, export_schema, import_schema, init, model::AliasKind,
    snapshot::SNAPSHOT_VERSION, validation::ValidationContext, StarterKind,
};

fn fresh_world_model_db() -> (tempfile::TempDir, GraphDb) {
    let dir = tempfile::tempdir().unwrap();
    let db = GraphDb::open(dir.path()).unwrap();
    init(&db, None, false).unwrap(); // WorldModel
    (dir, db)
}

// ── Basic round-trip ──────────────────────────────────────────────────────────

#[test]
fn snapshot_version_is_one() {
    let (_dir, db) = fresh_world_model_db();
    let snap = export_schema(&db).unwrap();
    assert_eq!(snap.snapshot_version, SNAPSHOT_VERSION);
    assert_eq!(snap.snapshot_version, 1);
}

#[test]
fn export_captures_world_model_classes() {
    let (_dir, db) = fresh_world_model_db();
    let snap = export_schema(&db).unwrap();
    let names: Vec<&str> = snap.classes.iter().map(|c| c.name.as_str()).collect();
    assert!(names.contains(&"Person"), "Person missing from snapshot");
    assert!(names.contains(&"Organization"), "Organization missing");
    assert!(names.contains(&"Task"), "Task missing");
    assert!(!snap.classes.is_empty());
}

#[test]
fn export_captures_world_model_relations() {
    let (_dir, db) = fresh_world_model_db();
    let snap = export_schema(&db).unwrap();
    assert!(!snap.relations.is_empty(), "no relations in snapshot");
    // WORKS_FOR should have domain=Person, range=Organization
    let wf = snap
        .relations
        .iter()
        .find(|r| r.name == "WORKS_FOR")
        .unwrap();
    assert_eq!(wf.domain, "Person");
    assert_eq!(wf.range, "Organization");
}

#[test]
fn export_captures_world_model_properties() {
    let (_dir, db) = fresh_world_model_db();
    let snap = export_schema(&db).unwrap();
    // World model seeds properties on Person and Task
    assert!(!snap.properties.is_empty(), "no properties in snapshot");
    let all_names: Vec<&str> = snap.properties.iter().map(|p| p.name.as_str()).collect();
    assert!(all_names.contains(&"name"), "name property missing");
}

// ── Custom schema elements survive round-trip ─────────────────────────────────

#[test]
fn snapshot_roundtrip_custom_class_and_alias() {
    let (dir_a, db_a) = fresh_world_model_db();

    // Add a custom class + alias
    sparrowdb_ontology_core::init::define_subclass(&db_a, "Task", "Concept").unwrap_or(());
    add_alias(&db_a, "Org", AliasKind::Class, "Organization").unwrap();

    let snap = export_schema(&db_a).unwrap();
    drop(db_a);
    drop(dir_a);

    // Import into a fresh DB
    let dir_b = tempfile::tempdir().unwrap();
    let db_b = GraphDb::open(dir_b.path()).unwrap();
    let result = import_schema(&db_b, &snap).unwrap();

    assert!(
        result.classes_imported >= 10,
        "expected world-model classes"
    );
    assert!(result.aliases_imported >= 1, "Org alias missing");

    // Alias should resolve in new DB
    let resolved = sparrowdb_ontology_core::resolve(&db_b, "Org", AliasKind::Class).unwrap();
    assert_eq!(resolved.canonical_name, "Organization");
    assert!(resolved.was_alias);
}

#[test]
fn snapshot_roundtrip_subclass_edge() {
    let (dir_a, db_a) = fresh_world_model_db();

    // Employee is a subclass of Person
    let employee =
        sparrowdb_ontology_core::model::OntologyClass::new("Employee", "An employed person");
    {
        use sparrowdb_storage::node_store::Value as StoreValue;
        let mut props = std::collections::HashMap::new();
        props.insert(
            "symbol_id".to_string(),
            StoreValue::Bytes(employee.symbol_id.as_bytes().to_vec()),
        );
        props.insert("name".to_string(), StoreValue::Bytes(b"Employee".to_vec()));
        props.insert(
            "description".to_string(),
            StoreValue::Bytes(b"An employed person".to_vec()),
        );
        props.insert("status".to_string(), StoreValue::Bytes(b"active".to_vec()));
        props.insert(
            "created_at".to_string(),
            StoreValue::Int64(employee.created_at),
        );
        props.insert(
            "updated_at".to_string(),
            StoreValue::Int64(employee.updated_at),
        );
        let mut tx = db_a.begin_write().unwrap();
        tx.merge_node("__SO_Class", props).unwrap();
        tx.commit().unwrap();
    }
    define_subclass(&db_a, "Employee", "Person").unwrap();

    let snap = export_schema(&db_a).unwrap();
    assert!(
        !snap.subclass_edges.is_empty(),
        "subclass edge not captured"
    );
    drop(db_a);
    drop(dir_a);

    let dir_b = tempfile::tempdir().unwrap();
    let db_b = GraphDb::open(dir_b.path()).unwrap();
    let result = import_schema(&db_b, &snap).unwrap();
    assert!(result.subclass_edges_imported >= 1);

    // is_subclass_of should work
    let ctx = ValidationContext::new(&db_b);
    assert!(ctx.is_subclass_of("Employee", "Person").unwrap());
    assert!(ctx.is_subclass_of("Employee", "Employee").unwrap());
    assert!(!ctx.is_subclass_of("Person", "Employee").unwrap());
}

#[test]
fn snapshot_roundtrip_property_with_unique_and_allowed_values() {
    let (dir_a, db_a) = fresh_world_model_db();

    add_property(
        &db_a,
        "Task",
        "severity_level",
        "string",
        false,
        false,
        Some(vec![
            "low".to_string(),
            "medium".to_string(),
            "high".to_string(),
        ]),
        None,
        None,
    )
    .unwrap();
    add_property(&db_a, "Person", "badge_id", "string", false, true, None, None, None).unwrap();

    let snap = export_schema(&db_a).unwrap();
    drop(db_a);
    drop(dir_a);

    let dir_b = tempfile::tempdir().unwrap();
    let db_b = GraphDb::open(dir_b.path()).unwrap();
    import_schema(&db_b, &snap).unwrap();

    // Verify allowed_values round-tripped
    let ctx = ValidationContext::new(&db_b);
    let task_sym = sparrowdb_ontology_core::resolve(&db_b, "Task", AliasKind::Class).unwrap();
    let props = ctx.get_properties_for_class(&task_sym.symbol_id).unwrap();
    let priority = props.iter().find(|p| p.name == "severity_level").unwrap();
    assert_eq!(
        priority.allowed_values.as_deref(),
        Some(["low".to_string(), "medium".to_string(), "high".to_string()].as_slice())
    );

    // Verify unique flag round-tripped
    let person_sym = sparrowdb_ontology_core::resolve(&db_b, "Person", AliasKind::Class).unwrap();
    let person_props = ctx.get_properties_for_class(&person_sym.symbol_id).unwrap();
    let badge = person_props.iter().find(|p| p.name == "badge_id").unwrap();
    assert!(badge.unique);
}

// ── JSON serialisation ────────────────────────────────────────────────────────

#[test]
fn snapshot_serialises_to_json_and_back() {
    let (_dir, db) = fresh_world_model_db();
    let snap = export_schema(&db).unwrap();

    let json = serde_json::to_string(&snap).expect("serialise failed");
    assert!(json.contains("\"snapshot_version\":1"));

    let restored: sparrowdb_ontology_core::SchemaSnapshot =
        serde_json::from_str(&json).expect("deserialise failed");
    assert_eq!(restored.classes.len(), snap.classes.len());
    assert_eq!(restored.relations.len(), snap.relations.len());
}

// ── Empty / blank DB ─────────────────────────────────────────────────────────

#[test]
fn export_blank_db_produces_empty_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let db = GraphDb::open(dir.path()).unwrap();
    init(&db, Some(StarterKind::Blank), false).unwrap();

    let snap = export_schema(&db).unwrap();
    assert!(snap.classes.is_empty());
    assert!(snap.relations.is_empty());
    assert!(snap.properties.is_empty());
    assert!(snap.aliases.is_empty());
    assert!(snap.subclass_edges.is_empty());
}

#[test]
fn import_empty_snapshot_into_fresh_db_is_noop() {
    use sparrowdb_ontology_core::snapshot::SchemaSnapshot;
    let dir = tempfile::tempdir().unwrap();
    let db = GraphDb::open(dir.path()).unwrap();
    let empty = SchemaSnapshot {
        snapshot_version: 1,
        exported_at: 0,
        classes: vec![],
        relations: vec![],
        properties: vec![],
        aliases: vec![],
        subclass_edges: vec![],
    };
    let result = import_schema(&db, &empty).unwrap();
    assert_eq!(result.classes_imported, 0);
    assert_eq!(result.aliases_imported, 0);
}
