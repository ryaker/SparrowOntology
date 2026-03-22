use std::collections::HashMap;

use sparrowdb::GraphDb;
use sparrowdb_ontology_core::*;

fn setup_test_db() -> (tempfile::TempDir, GraphDb) {
    let dir = tempfile::tempdir().expect("Failed to create temp dir");
    let db = GraphDb::open(dir.path().join("test.db")).expect("Failed to open GraphDb");
    (dir, db)
}

// ============================================================================
// Phase 1 Acceptance Criteria (24 total)
// ============================================================================

#[test]
fn ac_01_init_creates_exactly_10_classes() {
    let (_dir, db) = setup_test_db();
    let result = init(&db, false).expect("init failed");
    assert_eq!(
        result.classes_created, 10,
        "init() should create exactly 10 classes"
    );
}

#[test]
fn ac_02_init_creates_exactly_19_relations() {
    let (_dir, db) = setup_test_db();
    let result = init(&db, false).expect("init failed");
    assert_eq!(
        result.relations_created, 19,
        "init() should create exactly 19 relations"
    );
}

#[test]
fn ac_03_init_creates_exactly_22_properties() {
    let (_dir, db) = setup_test_db();
    let result = init(&db, false).expect("init failed");
    assert_eq!(
        result.properties_created, 22,
        "init() should create exactly 22 properties"
    );
}

#[test]
fn ac_04_init_second_call_returns_already_initialized() {
    let (_dir, db) = setup_test_db();
    let _result1 = init(&db, false).expect("First init failed");

    let result2 = init(&db, false);
    match result2 {
        Err(SoError::AlreadyInitialized {
            class_count,
            relation_count,
            property_count,
        }) => {
            assert_eq!(class_count, 10);
            assert_eq!(relation_count, 19);
            assert_eq!(property_count, 22);
        }
        _ => panic!("Expected AlreadyInitialized error"),
    }
}

#[test]
fn ac_05_init_force_true_reinitializes() {
    let (_dir, db) = setup_test_db();
    let _result1 = init(&db, false).expect("First init failed");
    let result2 = init(&db, true).expect("Forced reinit failed");

    assert_eq!(result2.classes_created, 10);
    assert_eq!(result2.relations_created, 19);
    assert_eq!(result2.properties_created, 22);
}

#[test]
fn ac_06_resolve_canonical_class() {
    let (_dir, db) = setup_test_db();
    let _result = init(&db, false).expect("init failed");

    let resolved = resolve(&db, "Person", AliasKind::Class).expect("resolve failed");
    assert_eq!(resolved.canonical_name, "Person");
    assert!(!resolved.was_alias);
    assert_eq!(resolved.kind, AliasKind::Class);
}

#[test]
fn ac_07_resolve_canonical_relation() {
    let (_dir, db) = setup_test_db();
    let _result = init(&db, false).expect("init failed");

    let resolved = resolve(&db, "WORKS_FOR", AliasKind::Relation).expect("resolve failed");
    assert_eq!(resolved.canonical_name, "WORKS_FOR");
    assert!(!resolved.was_alias);
    assert_eq!(resolved.kind, AliasKind::Relation);
}

#[test]
fn ac_08_resolve_unknown_symbol_error() {
    let (_dir, db) = setup_test_db();
    let _result = init(&db, false).expect("init failed");

    let result = resolve(&db, "NonExistentClass", AliasKind::Class);
    match result {
        Err(SoError::UnknownSymbol {
            symbol,
            kind,
            valid_options,
        }) => {
            assert_eq!(symbol, "NonExistentClass");
            assert!(!valid_options.is_empty());
        }
        _ => panic!("Expected UnknownSymbol error"),
    }
}

#[test]
fn ac_09_escape_cypher_string_apostrophe() {
    // The O'Reilly Inc apostrophe test
    let test_name = "O'Reilly Inc";
    let escaped = escape_cypher_string(test_name);
    assert_eq!(escaped, "O\\'Reilly Inc");
}

#[test]
fn ac_10_escape_cypher_string_backslash() {
    let test_path = "test\\path";
    let escaped = escape_cypher_string(test_path);
    assert_eq!(escaped, "test\\\\path");
}

#[test]
fn ac_11_validation_required_property_missing() {
    let (_dir, db) = setup_test_db();
    let _result = init(&db, false).expect("init failed");

    let ctx = ValidationContext::new(&db);
    let properties = HashMap::new();  // Empty: missing required 'name' property

    let result = ctx.validate_entity("Person", &properties, true);
    match result {
        Err(SoError::RequiredPropertyMissing { property, .. }) => {
            assert_eq!(property, "name");
        }
        _ => panic!("Expected RequiredPropertyMissing error"),
    }
}

#[test]
fn ac_12_validation_accepts_valid_entity() {
    let (_dir, db) = setup_test_db();
    let _result = init(&db, false).expect("init failed");

    let ctx = ValidationContext::new(&db);
    let mut properties = HashMap::new();
    properties.insert("name".to_string(), "Alice".to_string());

    let result = ctx.validate_entity("Person", &properties, true);
    assert!(result.is_ok(), "Valid entity should pass validation");
}

#[test]
fn ac_13_validation_rejects_reserved_property_prefix() {
    let (_dir, db) = setup_test_db();
    let _result = init(&db, false).expect("init failed");

    let ctx = ValidationContext::new(&db);
    let mut properties = HashMap::new();
    properties.insert("name".to_string(), "Alice".to_string());
    properties.insert("__so_invalid_property".to_string(), "value".to_string());

    let result = ctx.validate_entity("Person", &properties, true);
    match result {
        Err(SoError::ReservedProperty { property }) => {
            assert_eq!(property, "__so_invalid_property");
        }
        _ => panic!("Expected ReservedProperty error"),
    }
}

#[test]
fn ac_14_validation_allows_whitelisted_so_properties() {
    let (_dir, db) = setup_test_db();
    let _result = init(&db, false).expect("init failed");

    let ctx = ValidationContext::new(&db);
    let mut properties = HashMap::new();
    properties.insert("name".to_string(), "Alice".to_string());
    properties.insert("__so_source_label".to_string(), "Person".to_string());

    let result = ctx.validate_entity("Person", &properties, true);
    assert!(result.is_ok(), "Whitelisted __so_ property should be allowed");
}

#[test]
fn ac_15_validation_relationship_domain_check() {
    let (_dir, db) = setup_test_db();
    let _result = init(&db, false).expect("init failed");

    let ctx = ValidationContext::new(&db);

    // WORKS_FOR has domain Person and range Organization
    // This should pass: Person -> Organization
    let result = ctx.validate_relationship("WORKS_FOR", "Person", "Organization");
    assert!(result.is_ok(), "Valid relationship should pass");

    // This should fail: Organization -> Organization (wrong domain)
    let result2 = ctx.validate_relationship("WORKS_FOR", "Organization", "Organization");
    assert!(result2.is_err(), "Invalid domain should fail");
}

#[test]
fn ac_16_validation_relationship_range_check() {
    let (_dir, db) = setup_test_db();
    let _result = init(&db, false).expect("init failed");

    let ctx = ValidationContext::new(&db);

    // KNOWS has domain Person and range Person
    let result = ctx.validate_relationship("KNOWS", "Person", "Person");
    assert!(result.is_ok(), "Valid relationship should pass");

    // This should fail: Person -> Organization (wrong range)
    let result2 = ctx.validate_relationship("KNOWS", "Person", "Organization");
    assert!(result2.is_err(), "Invalid range should fail");
}

#[test]
fn ac_17_canonical_world_model_10_classes() {
    let classes = canonical_world_model();
    assert_eq!(classes.len(), 10);

    let names: Vec<&str> = classes
        .iter()
        .map(|c| c.name.as_str())
        .collect();

    assert!(names.contains(&"Person"));
    assert!(names.contains(&"Organization"));
    assert!(names.contains(&"Project"));
    assert!(names.contains(&"Task"));
    assert!(names.contains(&"Role"));
    assert!(names.contains(&"Event"));
    assert!(names.contains(&"Decision"));
    assert!(names.contains(&"Policy"));
    assert!(names.contains(&"Concept"));
    assert!(names.contains(&"Dependency"));
}

#[test]
fn ac_18_canonical_world_model_19_relations() {
    let relations = canonical_world_model_relations();
    assert_eq!(relations.len(), 19);

    let names: Vec<&str> = relations
        .iter()
        .map(|r| r.name.as_str())
        .collect();

    assert!(names.contains(&"WORKS_FOR"));
    assert!(names.contains(&"KNOWS"));
    assert!(names.contains(&"OWNS"));
    assert!(names.contains(&"DEPENDS_ON"));
}

#[test]
fn ac_19_canonical_world_model_22_properties() {
    let properties = canonical_world_model_properties();
    assert_eq!(properties.len(), 22);

    // Verify some key properties exist
    let person_props: Vec<&str> = properties
        .iter()
        .filter(|p| p.owner == "Person")
        .map(|p| p.name.as_str())
        .collect();

    assert!(person_props.contains(&"name"));
    assert!(person_props.contains(&"email"));
}

#[test]
fn ac_20_ontology_class_creation() {
    let class = OntologyClass::new("TestClass", Some("A test class"));
    assert_eq!(class.name, "TestClass");
    assert!(!class.symbol_id.is_empty());
    assert_eq!(class.description, Some("A test class".into()));
}

#[test]
fn ac_21_ontology_relation_creation() {
    let rel = OntologyRelation::new("TEST_REL", "Class1", "Class2", Some("Test relation"));
    assert_eq!(rel.name, "TEST_REL");
    assert_eq!(rel.domain, "Class1");
    assert_eq!(rel.range, "Class2");
    assert!(!rel.symbol_id.is_empty());
}

#[test]
fn ac_22_ontology_property_required() {
    let prop = OntologyProperty::required(
        "TestClass",
        OwnerKind::Class,
        "test_prop",
        PropertyType::String,
        Some("A required property"),
    );
    assert_eq!(prop.name, "test_prop");
    assert!(prop.is_required);
    assert_eq!(prop.datatype, PropertyType::String);
}

#[test]
fn ac_23_ontology_property_optional() {
    let prop = OntologyProperty::optional(
        "TestClass",
        OwnerKind::Class,
        "optional_prop",
        PropertyType::Integer,
        None,
    );
    assert_eq!(prop.name, "optional_prop");
    assert!(!prop.is_required);
    assert_eq!(prop.datatype, PropertyType::Integer);
}

#[test]
fn ac_24_error_types_compile() {
    // Verify all error types can be constructed

    let _err1 = SoError::ReservedNamespace {
        name: "test".to_string(),
    };

    let _err2 = SoError::UnknownSymbol {
        symbol: "test".to_string(),
        kind: "Class".to_string(),
        valid_options: "Option1, Option2".to_string(),
    };

    let _err3 = SoError::AlreadyInitialized {
        class_count: 10,
        relation_count: 19,
        property_count: 22,
    };

    let _err4 = SoError::CycleDetected {
        child: "A".to_string(),
        parent: "B".to_string(),
        edge_type: "SUBCLASS_OF".to_string(),
    };

    // All error constructors work
}

// ============================================================================
// Additional Integration Tests (beyond the 24 acceptance criteria)
// ============================================================================

#[test]
fn integration_list_canonical_names() {
    let (_dir, db) = setup_test_db();
    let _result = init(&db, false).expect("init failed");

    let names = list_canonical_names(&db, AliasKind::Class)
        .expect("list_canonical_names failed");

    assert_eq!(names.len(), 10);
    assert!(names.contains(&"Person".to_string()));
    assert!(names.contains(&"Organization".to_string()));
}

#[test]
fn integration_reserved_namespace_error() {
    let _err = SoError::ReservedNamespace {
        name: "__SO_BadName".to_string(),
    };
    assert!(_err.to_string().contains("ReservedNamespace"));
}

#[test]
fn integration_validation_context_creation() {
    let (_dir, db) = setup_test_db();
    let _result = init(&db, false).expect("init failed");

    let ctx = ValidationContext::new(&db);
    let mut props = HashMap::new();
    props.insert("name".to_string(), "Test".to_string());

    let result = ctx.validate_entity("Person", &props, true);
    assert!(result.is_ok());
}

#[test]
fn integration_property_types() {
    // Test all property types
    assert_eq!(PropertyType::String.to_string(), "String");
    assert_eq!(PropertyType::Integer.to_string(), "Integer");
    assert_eq!(PropertyType::Float.to_string(), "Float");
    assert_eq!(PropertyType::Boolean.to_string(), "Boolean");
    assert_eq!(PropertyType::DateTime.to_string(), "DateTime");
    assert_eq!(PropertyType::Json.to_string(), "Json");
}

#[test]
fn integration_alias_kind_display() {
    assert_eq!(AliasKind::Class.to_string(), "Class");
    assert_eq!(AliasKind::Relation.to_string(), "Relation");
    assert_eq!(AliasKind::Property.to_string(), "Property");
}
