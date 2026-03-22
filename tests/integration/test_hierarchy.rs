use std::collections::HashMap;

use sparrowdb::GraphDb;
use sparrowdb_ontology_core::{
    add_alias, define_subclass, init, model::AliasKind, validation::ValidationContext, SoError,
};
use sparrowdb_storage::node_store::Value as StoreValue;

fn sv(s: &str) -> StoreValue {
    StoreValue::Bytes(s.as_bytes().to_vec())
}

fn iv(n: i64) -> StoreValue {
    StoreValue::Int64(n)
}

fn seed_test_class(db: &GraphDb, symbol_id: &str, name: &str, description: &str) {
    let mut props = HashMap::new();
    props.insert("symbol_id".to_string(), sv(symbol_id));
    props.insert("name".to_string(), sv(name));
    props.insert("description".to_string(), sv(description));
    props.insert("status".to_string(), sv("active"));
    props.insert("created_at".to_string(), iv(0));
    props.insert("updated_at".to_string(), iv(0));

    let mut tx = db.begin_write().unwrap();
    tx.merge_node("__SO_Class", props).unwrap();
    tx.commit().unwrap();
}

/// Register `__SO_SUBCLASS_OF` in the catalog so that Cypher MATCH queries
/// using `[:__SO_SUBCLASS_OF*...]` do not fail with "unknown relationship type".
///
/// The first call to `define_subclass` runs a MATCH query via `check_no_cycle`
/// before any `__SO_SUBCLASS_OF` edge has been created. The Cypher binder
/// rejects any relationship type not already in the catalog. To pre-register
/// it we call `create_edge` — which writes the type to the catalog immediately
/// and non-transactionally — then drop the transaction without committing so
/// no spurious edge data is persisted.
fn register_subclass_rel_type(db: &GraphDb) {
    // We need two valid NodeIds. Merge two scratch nodes under a throwaway label.
    let (a, b) = {
        let mut tx = db.begin_write().unwrap();
        let mut p = HashMap::new();
        p.insert("_scratch".to_string(), iv(1));
        let na = tx.merge_node("__SO_RegScratch", p.clone()).unwrap();
        let nb = tx.merge_node("__SO_RegScratch", {
            let mut q = HashMap::new();
            q.insert("_scratch".to_string(), iv(2));
            q
        }).unwrap();
        tx.commit().unwrap();
        (na, nb)
    };
    // Call create_edge to register the rel type in the catalog (immediate,
    // non-transactional write). Drop the tx so the edge is not persisted.
    let mut tx = db.begin_write().unwrap();
    tx.create_edge(a, b, "__SO_SUBCLASS_OF", HashMap::new()).unwrap();
    drop(tx); // intentionally NOT committed — type registration is already done
}

fn initialized_db() -> (tempfile::TempDir, GraphDb) {
    let dir = tempfile::tempdir().unwrap();
    let db = GraphDb::open(dir.path()).unwrap();
    init(&db, None, false).unwrap();
    register_subclass_rel_type(&db);
    (dir, db)
}

#[test]
fn define_subclass_succeeds() {
    let (_dir, db) = initialized_db();
    // "Employee" is not in the world model — define it first as a class
    seed_test_class(&db, "emp-001", "Employee", "A person employed by an organization");
    define_subclass(&db, "Employee", "Person").unwrap();
}

#[test]
fn subclass_validate_relationship_passes() {
    let (_dir, db) = initialized_db();
    // Create Employee class and make it a subclass of Person
    seed_test_class(&db, "emp-001", "Employee", "A person employed by an organization");
    define_subclass(&db, "Employee", "Person").unwrap();

    let ctx = ValidationContext::new(&db);
    // WORKS_FOR domain=Person. Employee is a subclass of Person → should pass.
    let result = ctx.validate_relationship("WORKS_FOR", "Employee", "Organization");
    assert!(result.is_ok(), "expected Ok, got: {result:?}");
}

#[test]
fn define_subclass_cycle_returns_error() {
    let (_dir, db) = initialized_db();
    // Create A and B classes
    seed_test_class(&db, "cls-a", "ClassA", "");
    seed_test_class(&db, "cls-b", "ClassB", "");

    // A → B (A is subclass of B)
    define_subclass(&db, "ClassA", "ClassB").unwrap();

    // B → A would create a cycle: B → A → B
    let err = define_subclass(&db, "ClassB", "ClassA").unwrap_err();
    assert!(
        matches!(err, SoError::CycleDetected { .. }),
        "expected CycleDetected, got: {err:?}"
    );
}
