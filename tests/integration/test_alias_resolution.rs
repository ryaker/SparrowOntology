use sparrowdb::GraphDb;
use sparrowdb_ontology_core::{add_alias, init, model::AliasKind, resolve, SoError};

fn initialized_db() -> (tempfile::TempDir, GraphDb) {
    let dir = tempfile::tempdir().unwrap();
    let db = GraphDb::open(dir.path()).unwrap();
    init(&db, None, false).unwrap();
    // Seed expected aliases
    add_alias(&db, "EMPLOYED_BY", AliasKind::Relation, "WORKS_FOR").unwrap();
    add_alias(&db, "Human", AliasKind::Class, "Person").unwrap();
    (dir, db)
}

#[test]
fn resolve_employed_by_to_works_for() {
    let (_dir, db) = initialized_db();
    let sym = resolve(&db, "EMPLOYED_BY", AliasKind::Relation).unwrap();
    assert_eq!(sym.canonical_name, "WORKS_FOR");
    assert!(sym.was_alias);
}

#[test]
fn resolve_human_to_person() {
    let (_dir, db) = initialized_db();
    let sym = resolve(&db, "Human", AliasKind::Class).unwrap();
    assert_eq!(sym.canonical_name, "Person");
    assert!(sym.was_alias);
}

#[test]
fn resolve_canonical_name_directly() {
    let (_dir, db) = initialized_db();
    let sym = resolve(&db, "Person", AliasKind::Class).unwrap();
    assert_eq!(sym.canonical_name, "Person");
    assert!(!sym.was_alias);
}

#[test]
fn resolve_unknown_returns_error_with_valid() {
    let (_dir, db) = initialized_db();
    let err = resolve(&db, "Stakeholder", AliasKind::Class).unwrap_err();
    match err {
        SoError::UnknownSymbol { name, valid, .. } => {
            assert_eq!(name, "Stakeholder");
            assert!(!valid.is_empty(), "valid list must be non-empty");
        }
        other => panic!("expected UnknownSymbol, got: {other:?}"),
    }
}

#[test]
fn resolve_apostrophe_does_not_panic() {
    let (_dir, db) = initialized_db();
    let result = resolve(&db, "O'Reilly Inc", AliasKind::Class);
    // Should return UnknownSymbol, not panic or crash
    assert!(
        matches!(result, Err(SoError::UnknownSymbol { .. })),
        "expected UnknownSymbol for apostrophe input, got: {result:?}"
    );
}

#[test]
fn add_alias_succeeds() {
    let (_dir, db) = initialized_db();
    // Already added EMPLOYED_BY in setup — adding again is idempotent
    add_alias(&db, "hasEmployer", AliasKind::Relation, "WORKS_FOR").unwrap();
    let sym = resolve(&db, "hasEmployer", AliasKind::Relation).unwrap();
    assert_eq!(sym.canonical_name, "WORKS_FOR");
}

#[test]
fn add_alias_conflict_returns_error() {
    let (_dir, db) = initialized_db();
    // EMPLOYED_BY already points to WORKS_FOR — trying to point it to KNOWS should fail
    let err = add_alias(&db, "EMPLOYED_BY", AliasKind::Relation, "KNOWS").unwrap_err();
    assert!(
        matches!(err, SoError::AliasConflict { .. }),
        "expected AliasConflict, got: {err:?}"
    );
}

#[test]
fn same_alias_name_valid_for_class_and_relation() {
    let (_dir, db) = initialized_db();
    // "EMPLOYED_BY" is already a Relation alias
    // Adding it as a Class alias should NOT conflict
    add_alias(&db, "EMPLOYED_BY", AliasKind::Class, "Person").unwrap();
}
