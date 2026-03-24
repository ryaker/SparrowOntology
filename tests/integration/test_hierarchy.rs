use std::collections::HashMap;

use sparrowdb::GraphDb;
use sparrowdb_ontology_core::{
    add_property, define_subclass, init, validation::ValidationContext, PropertyValue, SoError,
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

fn initialized_db() -> (tempfile::TempDir, GraphDb) {
    let dir = tempfile::tempdir().unwrap();
    let db = GraphDb::open(dir.path()).unwrap();
    init(&db, None, false).unwrap();
    // check_no_cycle handles "unknown relationship type" as Ok (no cycle).
    // No pre-registration of __SO_SUBCLASS_OF needed.
    (dir, db)
}

#[test]
fn define_subclass_succeeds() {
    let (_dir, db) = initialized_db();
    seed_test_class(&db, "emp-001", "Employee", "A person employed by an organization");
    define_subclass(&db, "Employee", "Person").unwrap();
}

#[test]
fn subclass_validate_relationship_passes() {
    let (_dir, db) = initialized_db();
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
    seed_test_class(&db, "cls-a", "ClassA", "");
    seed_test_class(&db, "cls-b", "ClassB", "");

    // A → B (ClassA is subclass of ClassB)
    define_subclass(&db, "ClassA", "ClassB").unwrap();

    // B → A would create a cycle
    let err = define_subclass(&db, "ClassB", "ClassA").unwrap_err();
    assert!(
        matches!(err, SoError::CycleDetected { .. }),
        "expected CycleDetected, got: {err:?}"
    );
}

// ── Property inheritance tests ────────────────────────────────────────────────

/// Employee is a subclass of Person. Person requires `name`.
/// Creating an Employee without `name` should fail with RequiredPropertyMissing.
#[test]
fn inherited_required_property_enforced_on_create() {
    let (_dir, db) = initialized_db();
    seed_test_class(&db, "emp-001", "Employee", "A person employed by an organization");
    define_subclass(&db, "Employee", "Person").unwrap();
    // Employee has no own properties — it inherits `name` (required) from Person.

    let ctx = ValidationContext::new(&db);
    let empty: HashMap<String, PropertyValue> = HashMap::new();
    let err = ctx.validate_entity("Employee", &empty, true).unwrap_err();
    assert!(
        matches!(err, SoError::RequiredPropertyMissing { ref property, .. } if property == "name"),
        "expected RequiredPropertyMissing(name), got: {err:?}"
    );
}

/// Employee provides the inherited required `name` → create should succeed.
#[test]
fn inherited_required_property_satisfied_on_create() {
    let (_dir, db) = initialized_db();
    seed_test_class(&db, "emp-002", "Employee", "A person employed by an organization");
    define_subclass(&db, "Employee", "Person").unwrap();

    let ctx = ValidationContext::new(&db);
    let mut props: HashMap<String, PropertyValue> = HashMap::new();
    props.insert("name".to_string(), PropertyValue::String("Alice".to_string()));
    let result = ctx.validate_entity("Employee", &props, true);
    assert!(result.is_ok(), "expected Ok, got: {result:?}");
}

/// Child class can declare a property with the same name as a parent — child wins (no conflict).
#[test]
fn child_property_overrides_parent_on_validate() {
    let (_dir, db) = initialized_db();
    seed_test_class(&db, "emp-003", "Employee", "");
    define_subclass(&db, "Employee", "Person").unwrap();
    // Employee also declares `name` explicitly as optional — overrides Person's required `name`.
    add_property(&db, "Employee", "name", "string", false, false, None).unwrap();

    let ctx = ValidationContext::new(&db);
    let empty: HashMap<String, PropertyValue> = HashMap::new();
    // Should succeed because Employee's own `name` is optional (not required).
    let result = ctx.validate_entity("Employee", &empty, true);
    assert!(result.is_ok(), "expected Ok (child overrides parent required), got: {result:?}");
}

/// Multi-level inheritance: GradStudent → Student → Person.
/// Person requires `name`. GradStudent with no properties → RequiredPropertyMissing.
#[test]
fn multi_level_inheritance_enforces_grandparent_required_property() {
    let (_dir, db) = initialized_db();
    seed_test_class(&db, "stu-001", "Student", "A person enrolled in a course");
    seed_test_class(&db, "grad-001", "GradStudent", "A graduate-level student");
    define_subclass(&db, "Student", "Person").unwrap();
    define_subclass(&db, "GradStudent", "Student").unwrap();

    let ctx = ValidationContext::new(&db);
    let empty: HashMap<String, PropertyValue> = HashMap::new();
    let err = ctx.validate_entity("GradStudent", &empty, true).unwrap_err();
    assert!(
        matches!(err, SoError::RequiredPropertyMissing { ref property, .. } if property == "name"),
        "expected RequiredPropertyMissing(name) from grandparent Person, got: {err:?}"
    );
}
