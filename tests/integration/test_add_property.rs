use std::collections::HashMap;

use sparrowdb::GraphDb;
use sparrowdb_ontology_core::{
    add_property, init,
    model::{PropertyType, PropertyValue},
    validation::ValidationContext,
    SoError,
};
fn initialized_db() -> (tempfile::TempDir, GraphDb) {
    let dir = tempfile::tempdir().unwrap();
    let db = GraphDb::open(dir.path()).unwrap();
    init(&db, None, false).unwrap();
    (dir, db)
}

#[test]
fn add_property_to_existing_class() {
    let (_dir, db) = initialized_db();
    // Person already exists in the world model
    let prop = add_property(&db, "Person", "age", "int64", false, false, None, None, None).unwrap();
    assert_eq!(prop.name, "age");
    assert_eq!(prop.datatype, PropertyType::Int64);
    assert!(!prop.required);
    assert_eq!(prop.owner_name, "Person");
}

#[test]
fn add_required_property() {
    let (_dir, db) = initialized_db();
    let prop = add_property(&db, "Task", "deadline", "date", true, false, None, None, None).unwrap();
    assert!(prop.required);
}

#[test]
fn add_property_variant_datatype() {
    let (_dir, db) = initialized_db();
    let prop = add_property(&db, "Concept", "metadata", "variant", false, false, None, None, None).unwrap();
    assert_eq!(prop.datatype, PropertyType::Variant);
}

#[test]
fn add_property_unknown_class_returns_error() {
    let (_dir, db) = initialized_db();
    let err =
        add_property(&db, "NonExistentClass", "foo", "string", false, false, None, None, None).unwrap_err();
    assert!(
        matches!(err, SoError::UnknownSymbol { .. }),
        "expected UnknownSymbol, got: {err:?}"
    );
}

#[test]
fn add_property_reserved_name_returns_error() {
    let (_dir, db) = initialized_db();
    let err = add_property(&db, "Person", "__so_secret", "string", false, false, None, None, None).unwrap_err();
    assert!(
        matches!(err, SoError::ReservedProperty(_)),
        "expected ReservedProperty, got: {err:?}"
    );
}

#[test]
fn add_property_duplicate_returns_error() {
    let (_dir, db) = initialized_db();
    add_property(&db, "Person", "nickname", "string", false, false, None, None, None).unwrap();
    let err = add_property(&db, "Person", "nickname", "string", false, false, None, None, None).unwrap_err();
    assert!(
        matches!(err, SoError::DuplicateProperty { .. }),
        "expected DuplicateProperty, got: {err:?}"
    );
}

#[test]
fn add_property_visible_in_validation_context() {
    let (_dir, db) = initialized_db();
    add_property(&db, "Person", "nickname", "string", false, false, None, None, None).unwrap();

    let ctx = ValidationContext::new(&db);
    // Resolve Person's symbol_id first
    let sym = sparrowdb_ontology_core::resolve(
        &db,
        "Person",
        sparrowdb_ontology_core::model::AliasKind::Class,
    )
    .unwrap();
    let props = ctx.get_properties_for_class(&sym.symbol_id).unwrap();
    assert!(
        props.iter().any(|p| p.name == "nickname"),
        "nickname should be in declared properties for Person"
    );
}

#[test]
fn add_property_validates_entity_with_new_prop() {
    let (_dir, db) = initialized_db();
    add_property(&db, "Task", "due_date", "date", false, false, None, None, None).unwrap();

    let ctx = ValidationContext::new(&db);
    let mut props = HashMap::new();
    props.insert(
        "name".to_string(),
        PropertyValue::String("My task".to_string()),
    );
    props.insert(
        "due_date".to_string(),
        PropertyValue::String("2026-03-22".to_string()),
    );

    // Should pass — due_date is now declared as a date property
    let result = ctx.validate_entity("Task", &props, true);
    assert!(result.is_ok(), "expected Ok, got: {result:?}");
}

#[test]
fn add_property_via_alias_resolves_owner() {
    let (_dir, db) = initialized_db();
    sparrowdb_ontology_core::add_alias(
        &db,
        "Org",
        sparrowdb_ontology_core::model::AliasKind::Class,
        "Organization",
    )
    .unwrap();

    // Add property using the alias "Org" as the owner
    let prop = add_property(&db, "Org", "industry", "string", false, false, None, None, None).unwrap();
    // Owner resolves to the canonical name
    assert_eq!(prop.owner_name, "Organization");
}

#[test]
fn add_property_unique_stores_flag() {
    let (_dir, db) = initialized_db();
    // Use "badge_id" — not pre-seeded on Person in world model
    let prop = add_property(&db, "Person", "badge_id", "string", false, true, None, None, None).unwrap();
    assert!(prop.unique, "expected unique=true");

    // Verify unique flag round-trips through storage
    let ctx = ValidationContext::new(&db);
    let declared = ctx.get_properties_for_class(&prop.owner_symbol_id).unwrap();
    let found = declared.iter().find(|p| p.name == "badge_id").unwrap();
    assert!(
        found.unique,
        "unique flag should round-trip through storage"
    );
}

#[test]
fn add_property_allowed_values_enforced() {
    let (_dir, db) = initialized_db();
    // Use "result_code" — not pre-seeded on Task in world model
    add_property(
        &db,
        "Task",
        "result_code",
        "string",
        false,
        false,
        Some(vec![
            "pass".to_string(),
            "fail".to_string(),
            "skip".to_string(),
        ]),
        None,
        None,
    )
    .unwrap();

    let ctx = ValidationContext::new(&db);

    // Valid value — should pass
    let mut props = HashMap::new();
    props.insert(
        "name".to_string(),
        PropertyValue::String("Fix bug".to_string()),
    );
    props.insert(
        "result_code".to_string(),
        PropertyValue::String("pass".to_string()),
    );
    assert!(ctx.validate_entity("Task", &props, true).is_ok());

    // Invalid value — should fail with EnumViolation
    props.insert(
        "result_code".to_string(),
        PropertyValue::String("unknown".to_string()),
    );
    let err = ctx.validate_entity("Task", &props, true).unwrap_err();
    assert!(
        matches!(err, SoError::EnumViolation { ref value, .. } if value == "unknown"),
        "expected EnumViolation, got: {err:?}"
    );
}

#[test]
fn add_property_allowed_values_round_trips() {
    let (_dir, db) = initialized_db();
    let allowed = vec!["draft".to_string(), "published".to_string()];
    let prop = add_property(
        &db,
        "Decision",
        "state",
        "string",
        false,
        false,
        Some(allowed.clone()),
        None,
        None,
    )
    .unwrap();
    assert_eq!(prop.allowed_values.as_deref(), Some(allowed.as_slice()));

    // Verify it round-trips through storage
    let ctx = ValidationContext::new(&db);
    let declared = ctx.get_properties_for_class(&prop.owner_symbol_id).unwrap();
    let found = declared.iter().find(|p| p.name == "state").unwrap();
    assert_eq!(found.allowed_values.as_deref(), Some(allowed.as_slice()));
}
