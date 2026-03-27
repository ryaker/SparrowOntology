use sparrowdb::GraphDb;
use sparrowdb_execution::Value as ExecValue;
use sparrowdb_ontology_core::{init, StarterKind};

fn open_db() -> (tempfile::TempDir, GraphDb) {
    let dir = tempfile::tempdir().unwrap();
    let db = GraphDb::open(dir.path()).unwrap();
    (dir, db)
}

/// Returns sorted class names present in the DB after init.
fn class_names(db: &GraphDb) -> Vec<String> {
    let result = db.execute("MATCH (c:__SO_Class) RETURN c.name").unwrap();
    let mut names: Vec<String> = result
        .rows
        .iter()
        .filter_map(|row| {
            row.first().and_then(|v| {
                if let ExecValue::String(s) = v {
                    Some(s.clone())
                } else {
                    None
                }
            })
        })
        .collect();
    names.sort();
    names
}

/// Returns sorted relation names present in the DB after init.
fn relation_names(db: &GraphDb) -> Vec<String> {
    let result = db.execute("MATCH (r:__SO_Relation) RETURN r.name").unwrap();
    let mut names: Vec<String> = result
        .rows
        .iter()
        .filter_map(|row| {
            row.first().and_then(|v| {
                if let ExecValue::String(s) = v {
                    Some(s.clone())
                } else {
                    None
                }
            })
        })
        .collect();
    names.sort();
    names
}

// ── PersonalKnowledge ─────────────────────────────────────────────────────────

#[test]
fn personal_knowledge_creates_expected_classes() {
    let (_dir, db) = open_db();
    let result = init(&db, Some(StarterKind::PersonalKnowledge), false).unwrap();
    assert_eq!(result.classes_created, 5);

    let names = class_names(&db);
    for expected in &["Concept", "Document", "Event", "Location", "Person"] {
        assert!(
            names.contains(&expected.to_string()),
            "missing class {expected}: {names:?}"
        );
    }
}

#[test]
fn personal_knowledge_creates_expected_relations() {
    let (_dir, db) = open_db();
    init(&db, Some(StarterKind::PersonalKnowledge), false).unwrap();

    let names = relation_names(&db);
    for expected in &[
        "KNOWS",
        "LOCATED_IN",
        "OCCURRED_AT",
        "PART_OF",
        "RELATED_TO",
    ] {
        assert!(
            names.contains(&expected.to_string()),
            "missing relation {expected}: {names:?}"
        );
    }
}

#[test]
fn personal_knowledge_starter_kind_in_result() {
    let (_dir, db) = open_db();
    let result = init(&db, Some(StarterKind::PersonalKnowledge), false).unwrap();
    assert!(
        matches!(result.starter, StarterKind::PersonalKnowledge),
        "unexpected starter kind: {:?}",
        result.starter
    );
}

// ── ProfessionalNetwork ───────────────────────────────────────────────────────

#[test]
fn professional_network_creates_expected_classes() {
    let (_dir, db) = open_db();
    let result = init(&db, Some(StarterKind::ProfessionalNetwork), false).unwrap();
    assert_eq!(result.classes_created, 5);

    let names = class_names(&db);
    for expected in &["Event", "Organization", "Person", "Project", "Role"] {
        assert!(
            names.contains(&expected.to_string()),
            "missing class {expected}: {names:?}"
        );
    }
}

#[test]
fn professional_network_creates_expected_relations() {
    let (_dir, db) = open_db();
    init(&db, Some(StarterKind::ProfessionalNetwork), false).unwrap();

    let names = relation_names(&db);
    for expected in &[
        "DEPENDS_ON",
        "HAS_ROLE",
        "LEADS",
        "MEMBER_OF",
        "PARTICIPATED_IN",
        "WORKS_FOR",
    ] {
        assert!(
            names.contains(&expected.to_string()),
            "missing relation {expected}: {names:?}"
        );
    }
}

#[test]
fn professional_network_starter_kind_in_result() {
    let (_dir, db) = open_db();
    let result = init(&db, Some(StarterKind::ProfessionalNetwork), false).unwrap();
    assert!(
        matches!(result.starter, StarterKind::ProfessionalNetwork),
        "unexpected starter kind: {:?}",
        result.starter
    );
}

// ── ResearchNotes ─────────────────────────────────────────────────────────────

#[test]
fn research_notes_creates_expected_classes() {
    let (_dir, db) = open_db();
    let result = init(&db, Some(StarterKind::ResearchNotes), false).unwrap();
    assert_eq!(result.classes_created, 5);

    let names = class_names(&db);
    for expected in &["Asset", "Claim", "Concept", "Document", "Person"] {
        assert!(
            names.contains(&expected.to_string()),
            "missing class {expected}: {names:?}"
        );
    }
}

#[test]
fn research_notes_creates_expected_relations() {
    let (_dir, db) = open_db();
    init(&db, Some(StarterKind::ResearchNotes), false).unwrap();

    let names = relation_names(&db);
    for expected in &[
        "AUTHORED",
        "CITES",
        "CONTRADICTS",
        "DERIVED_FROM",
        "SUPPORTS",
        "TAGGED_WITH",
    ] {
        assert!(
            names.contains(&expected.to_string()),
            "missing relation {expected}: {names:?}"
        );
    }
}

#[test]
fn research_notes_starter_kind_in_result() {
    let (_dir, db) = open_db();
    let result = init(&db, Some(StarterKind::ResearchNotes), false).unwrap();
    assert!(
        matches!(result.starter, StarterKind::ResearchNotes),
        "unexpected starter kind: {:?}",
        result.starter
    );
}
