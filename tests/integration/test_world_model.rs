use sparrowdb::GraphDb;
use sparrowdb_ontology_core::{init, model::*, SoError};

fn open_db() -> (tempfile::TempDir, GraphDb) {
    let dir = tempfile::tempdir().unwrap();
    let db = GraphDb::open(dir.path()).unwrap();
    (dir, db)
}

#[test]
fn init_creates_10_classes() {
    let (_dir, db) = open_db();
    let result = init(&db, None, false).unwrap();
    assert_eq!(result.classes_created, 10);
}

#[test]
fn init_creates_19_relations() {
    let (_dir, db) = open_db();
    let result = init(&db, None, false).unwrap();
    assert_eq!(result.relations_created, 19);
}

#[test]
fn init_creates_22_properties() {
    let (_dir, db) = open_db();
    let result = init(&db, None, false).unwrap();
    assert_eq!(result.properties_created, 22);
}

#[test]
fn init_returns_already_initialized() {
    let (_dir, db) = open_db();
    init(&db, None, false).unwrap();
    let err = init(&db, None, false).unwrap_err();
    assert!(
        matches!(err, SoError::AlreadyInitialized),
        "expected AlreadyInitialized, got: {err:?}"
    );
}

#[test]
fn init_force_wipes_and_recreates() {
    let (_dir, db) = open_db();
    init(&db, None, false).unwrap();
    let result = init(&db, None, true).unwrap();
    assert_eq!(result.classes_created, 10);
    assert_eq!(result.relations_created, 19);
    assert_eq!(result.properties_created, 22);
}

#[test]
fn canonical_world_model_has_10_classes() {
    assert_eq!(canonical_world_model().len(), 10);
}

#[test]
fn canonical_world_model_has_19_relations() {
    assert_eq!(canonical_world_model_relations().len(), 19);
}

#[test]
fn canonical_world_model_has_22_properties() {
    assert_eq!(canonical_world_model_properties().len(), 22);
}
