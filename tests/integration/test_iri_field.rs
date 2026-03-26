use sparrowdb::GraphDb;
use sparrowdb_ontology_core::{
    export_schema, import_schema, init,
    model::{OntologyClass, OntologyRelation},
    snapshot::SchemaSnapshot,
    StarterKind,
};
use sparrowdb_storage::node_store::Value as StoreValue;

fn fresh_blank_db() -> (tempfile::TempDir, GraphDb) {
    let dir = tempfile::tempdir().unwrap();
    let db = GraphDb::open(dir.path()).unwrap();
    init(&db, Some(StarterKind::Blank), false).unwrap();
    (dir, db)
}

/// Seed a class node with an optional iri using WriteTx (mirrors init.rs pattern).
fn seed_class_with_iri(db: &GraphDb, c: &OntologyClass) {
    let iri = c.iri.as_deref().unwrap_or("");
    let mut props = std::collections::HashMap::new();
    props.insert("symbol_id".to_string(), StoreValue::Bytes(c.symbol_id.as_bytes().to_vec()));
    props.insert("name".to_string(), StoreValue::Bytes(c.name.as_bytes().to_vec()));
    props.insert(
        "description".to_string(),
        StoreValue::Bytes(c.description.as_deref().unwrap_or("").as_bytes().to_vec()),
    );
    props.insert("status".to_string(), StoreValue::Bytes(b"active".to_vec()));
    props.insert("iri".to_string(), StoreValue::Bytes(iri.as_bytes().to_vec()));
    props.insert("created_at".to_string(), StoreValue::Int64(c.created_at));
    props.insert("updated_at".to_string(), StoreValue::Int64(c.updated_at));
    let mut tx = db.begin_write().unwrap();
    tx.merge_node("__SO_Class", props).unwrap();
    tx.commit().unwrap();
}

/// Seed a relation node with an optional iri using WriteTx.
fn seed_relation_with_iri(db: &GraphDb, r: &OntologyRelation) {
    let iri = r.iri.as_deref().unwrap_or("");
    let mut props = std::collections::HashMap::new();
    props.insert("symbol_id".to_string(), StoreValue::Bytes(r.symbol_id.as_bytes().to_vec()));
    props.insert("name".to_string(), StoreValue::Bytes(r.name.as_bytes().to_vec()));
    props.insert(
        "description".to_string(),
        StoreValue::Bytes(r.description.as_deref().unwrap_or("").as_bytes().to_vec()),
    );
    props.insert("status".to_string(), StoreValue::Bytes(b"active".to_vec()));
    props.insert("directed".to_string(), StoreValue::Int64(if r.directed { 1 } else { 0 }));
    props.insert("iri".to_string(), StoreValue::Bytes(iri.as_bytes().to_vec()));
    props.insert("created_at".to_string(), StoreValue::Int64(r.created_at));
    props.insert("updated_at".to_string(), StoreValue::Int64(r.updated_at));
    let mut tx = db.begin_write().unwrap();
    tx.merge_node("__SO_Relation", props).unwrap();
    tx.commit().unwrap();
}

// ── iri is None by default ────────────────────────────────────────────────────

#[test]
fn iri_field_is_none_by_default_on_class() {
    let c = OntologyClass::new("TestClass", "A test class");
    assert!(c.iri.is_none(), "iri should be None by default on OntologyClass");
}

#[test]
fn iri_field_is_none_by_default_on_relation() {
    let r = OntologyRelation::new("TEST_REL", "ClassA", "ClassB");
    assert!(r.iri.is_none(), "iri should be None by default on OntologyRelation");
}

#[test]
fn iri_field_none_in_exported_snapshot_when_not_set() {
    let (_dir, db) = fresh_blank_db();

    // Seed a class without iri
    let c = OntologyClass::new("Widget", "A widget class");
    seed_class_with_iri(&db, &c);

    let snap = export_schema(&db).unwrap();
    let widget = snap.classes.iter().find(|x| x.name == "Widget").unwrap();
    assert!(widget.iri.is_none(), "iri should be None when not set");
}

// ── iri round-trips through snapshot ─────────────────────────────────────────

#[test]
fn iri_field_roundtrips_through_snapshot_for_class() {
    let (dir_a, db_a) = fresh_blank_db();

    let mut c = OntologyClass::new("Person", "A human individual");
    c.iri = Some("https://schema.org/Person".to_string());
    seed_class_with_iri(&db_a, &c);

    let snap = export_schema(&db_a).unwrap();
    let person = snap.classes.iter().find(|x| x.name == "Person").unwrap();
    assert_eq!(
        person.iri.as_deref(),
        Some("https://schema.org/Person"),
        "iri not captured by export_schema"
    );

    drop(db_a);
    drop(dir_a);

    // Import into a fresh DB and verify iri is preserved
    let dir_b = tempfile::tempdir().unwrap();
    let db_b = GraphDb::open(dir_b.path()).unwrap();
    import_schema(&db_b, &snap).unwrap();

    let snap2 = export_schema(&db_b).unwrap();
    let person2 = snap2.classes.iter().find(|x| x.name == "Person").unwrap();
    assert_eq!(
        person2.iri.as_deref(),
        Some("https://schema.org/Person"),
        "iri not preserved after import_schema round-trip"
    );
}

#[test]
fn iri_field_roundtrips_through_snapshot_for_relation() {
    let (dir_a, db_a) = fresh_blank_db();

    // Need domain + range classes first
    let domain = OntologyClass::new("Person", "A person");
    let range = OntologyClass::new("Organization", "An org");
    seed_class_with_iri(&db_a, &domain);
    seed_class_with_iri(&db_a, &range);

    let mut r = OntologyRelation::new("WORKS_FOR", "Person", "Organization");
    r.iri = Some("https://schema.org/worksFor".to_string());
    seed_relation_with_iri(&db_a, &r);

    let snap = export_schema(&db_a).unwrap();
    // Relations without DOMAIN/RANGE edges will have empty domain/range strings,
    // but the iri field should still be captured.
    let wf = snap.relations.iter().find(|x| x.name == "WORKS_FOR").unwrap();
    assert_eq!(
        wf.iri.as_deref(),
        Some("https://schema.org/worksFor"),
        "relation iri not captured by export_schema"
    );

    drop(db_a);
    drop(dir_a);

    let dir_b = tempfile::tempdir().unwrap();
    let db_b = GraphDb::open(dir_b.path()).unwrap();
    import_schema(&db_b, &snap).unwrap();

    let snap2 = export_schema(&db_b).unwrap();
    let wf2 = snap2.relations.iter().find(|x| x.name == "WORKS_FOR").unwrap();
    assert_eq!(
        wf2.iri.as_deref(),
        Some("https://schema.org/worksFor"),
        "relation iri not preserved after import_schema round-trip"
    );
}

// ── iri is backward-compatible: snapshots without iri deserialise cleanly ─────

#[test]
fn snapshot_without_iri_field_deserialises_with_none() {
    // A JSON snapshot that has no "iri" key at all (older format)
    let json = r#"{
        "snapshot_version": 1,
        "exported_at": 1000000,
        "classes": [
            {
                "symbol_id": "abc-123",
                "name": "LegacyClass",
                "status": "Active",
                "created_at": 0,
                "updated_at": 0
            }
        ],
        "relations": [],
        "properties": [],
        "aliases": [],
        "subclass_edges": []
    }"#;

    let snap: SchemaSnapshot = serde_json::from_str(json).expect("should deserialise without iri field");
    let lc = snap.classes.iter().find(|c| c.name == "LegacyClass").unwrap();
    assert!(lc.iri.is_none(), "iri should default to None when absent from JSON");
}
