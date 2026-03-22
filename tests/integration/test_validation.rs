use std::collections::HashMap;

use sparrowdb::GraphDb;
use sparrowdb_ontology_core::{add_alias, init, model::*, validation::ValidationContext, SoError};

fn initialized_db() -> (tempfile::TempDir, GraphDb) {
    let dir = tempfile::tempdir().unwrap();
    let db = GraphDb::open(dir.path()).unwrap();
    init(&db, None, false).unwrap();
    (dir, db)
}

fn props(pairs: &[(&str, PropertyValue)]) -> HashMap<String, PropertyValue> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()
}

#[test]
fn validate_entity_person_with_name_succeeds() {
    let (_dir, db) = initialized_db();
    let ctx = ValidationContext::new(&db);
    let result = ctx.validate_entity(
        "Person",
        &props(&[("name", PropertyValue::String("Alice".into()))]),
        true,
    );
    assert!(result.is_ok(), "expected Ok, got: {result:?}");
}

#[test]
fn validate_entity_unknown_class_returns_error() {
    let (_dir, db) = initialized_db();
    let ctx = ValidationContext::new(&db);
    let err = ctx
        .validate_entity("Stakeholder", &props(&[]), true)
        .unwrap_err();
    assert!(
        matches!(err, SoError::UnknownSymbol { .. }),
        "expected UnknownSymbol, got: {err:?}"
    );
}

#[test]
fn validate_entity_missing_required_name() {
    let (_dir, db) = initialized_db();
    let ctx = ValidationContext::new(&db);
    let err = ctx.validate_entity("Person", &props(&[]), true).unwrap_err();
    assert!(
        matches!(err, SoError::RequiredPropertyMissing { ref property, .. } if property == "name"),
        "expected RequiredPropertyMissing for 'name', got: {err:?}"
    );
}

#[test]
fn validate_entity_type_mismatch_name_as_int() {
    let (_dir, db) = initialized_db();
    let ctx = ValidationContext::new(&db);
    let err = ctx
        .validate_entity(
            "Person",
            &props(&[("name", PropertyValue::Int64(42))]),
            true,
        )
        .unwrap_err();
    assert!(
        matches!(err, SoError::TypeMismatch { ref property, .. } if property == "name"),
        "expected TypeMismatch for 'name', got: {err:?}"
    );
}

#[test]
fn validate_entity_allowed_so_source_label_key() {
    let (_dir, db) = initialized_db();
    let ctx = ValidationContext::new(&db);
    // __so_source_label is allowed
    let result = ctx.validate_entity(
        "Person",
        &props(&[
            ("name", PropertyValue::String("Alice".into())),
            ("__so_source_label", PropertyValue::String("Human".into())),
        ]),
        true,
    );
    assert!(result.is_ok(), "expected Ok, got: {result:?}");
}

#[test]
fn validate_entity_reserved_so_key_returns_error() {
    let (_dir, db) = initialized_db();
    let ctx = ValidationContext::new(&db);
    let err = ctx
        .validate_entity(
            "Person",
            &props(&[
                ("name", PropertyValue::String("Alice".into())),
                ("__so_evil", PropertyValue::String("hack".into())),
            ]),
            true,
        )
        .unwrap_err();
    assert!(
        matches!(err, SoError::ReservedProperty(ref k) if k == "__so_evil"),
        "expected ReservedProperty(__so_evil), got: {err:?}"
    );
}

#[test]
fn validate_relationship_valid_domain_range() {
    let (_dir, db) = initialized_db();
    let ctx = ValidationContext::new(&db);
    let result = ctx.validate_relationship("WORKS_FOR", "Person", "Organization");
    assert!(result.is_ok(), "expected Ok, got: {result:?}");
}

#[test]
fn validate_relationship_domain_violation() {
    let (_dir, db) = initialized_db();
    let ctx = ValidationContext::new(&db);
    let err = ctx
        .validate_relationship("WORKS_FOR", "Organization", "Person")
        .unwrap_err();
    assert!(
        matches!(err, SoError::DomainViolation { .. }),
        "expected DomainViolation, got: {err:?}"
    );
}

#[test]
fn validate_relationship_alias_resolves() {
    let (_dir, db) = initialized_db();
    add_alias(&db, "EMPLOYED_BY", AliasKind::Relation, "WORKS_FOR").unwrap();
    let ctx = ValidationContext::new(&db);
    // EMPLOYED_BY is an alias for WORKS_FOR — should succeed
    let result = ctx.validate_relationship("EMPLOYED_BY", "Person", "Organization");
    assert!(result.is_ok(), "expected Ok, got: {result:?}");
}
