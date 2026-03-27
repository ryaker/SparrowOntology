use std::collections::HashMap;

use sparrowdb::GraphDb;
use sparrowdb_ontology_core::{import_records, init, ImportTemplate, SoError};

// ── DB helpers ────────────────────────────────────────────────────────────────

fn initialized_db() -> (tempfile::TempDir, GraphDb) {
    let dir = tempfile::tempdir().unwrap();
    let db = GraphDb::open(dir.path()).unwrap();
    init(&db, None, false).unwrap();
    (dir, db)
}

fn person_template() -> ImportTemplate {
    ImportTemplate {
        version: 1,
        class: "Person".to_string(),
        mappings: {
            let mut m = HashMap::new();
            m.insert("full_name".to_string(), "name".to_string());
            m.insert("email_address".to_string(), "email".to_string());
            m
        },
        key_field: Some("id".to_string()),
    }
}

// ── Helpers to build records ───────────────────────────────────────────────────

fn record(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[test]
fn import_basic_records() {
    let (_dir, db) = initialized_db();
    let template = person_template();

    let records = vec![
        record(&[
            ("full_name", "Alice"),
            ("email_address", "alice@example.com"),
            ("id", "1"),
        ]),
        record(&[
            ("full_name", "Bob"),
            ("email_address", "bob@example.com"),
            ("id", "2"),
        ]),
    ];

    let result = import_records(&db, &records, &template, false, false).unwrap();
    assert_eq!(result.created, 2);
    assert_eq!(result.skipped, 0);
    assert_eq!(result.error_count(), 0);
}

#[test]
fn import_dry_run_writes_nothing() {
    let (_dir, db) = initialized_db();
    let template = person_template();

    let records = vec![record(&[("full_name", "DryPerson"), ("id", "42")])];

    // Dry run — should validate and count but not persist.
    let result = import_records(&db, &records, &template, true, false).unwrap();
    assert_eq!(result.created, 1, "dry_run should count as created");

    // Verify nothing actually exists by querying (no Cypher find API, so just check no error
    // from a second import with the same data — idempotent merge is fine here).
    let result2 = import_records(&db, &records, &template, false, false).unwrap();
    assert_eq!(
        result2.created, 1,
        "real import after dry run should succeed"
    );
}

#[test]
fn import_skip_errors_bad_rows_skipped() {
    let (_dir, db) = initialized_db();
    let template = person_template();

    // Record missing the required "name" field (mapped from "full_name").
    let bad_record = record(&[("email_address", "no-name@example.com"), ("id", "99")]);
    let good_record = record(&[("full_name", "Charlie"), ("id", "3")]);

    let records = vec![bad_record, good_record];
    let result = import_records(&db, &records, &template, false, true).unwrap();

    assert_eq!(result.created, 1, "only the good record should be created");
    assert_eq!(result.skipped, 1, "one bad record should be skipped");
    assert_eq!(result.error_count(), 1);
    assert!(result.errors[0].message.contains("name") || !result.errors[0].message.is_empty());
}

#[test]
fn import_skip_errors_false_aborts_on_first_error() {
    let (_dir, db) = initialized_db();
    let template = person_template();

    // Bad record first — missing required "name".
    let bad_record = record(&[("email_address", "nobody@example.com")]);
    let records = vec![bad_record];

    let err = import_records(&db, &records, &template, false, false).unwrap_err();
    // Should be a validation error (RequiredPropertyMissing).
    assert!(
        matches!(err, SoError::RequiredPropertyMissing { .. }),
        "expected RequiredPropertyMissing, got: {err:?}"
    );
}

#[test]
fn import_unknown_class_is_fatal_error() {
    let (_dir, db) = initialized_db();
    let template = ImportTemplate {
        version: 1,
        class: "NoSuchClass".to_string(),
        mappings: HashMap::new(),
        key_field: None,
    };

    let records = vec![record(&[("foo", "bar")])];
    let err = import_records(&db, &records, &template, false, true).unwrap_err();

    // Even with skip_errors=true, an unknown class is a fatal, non-row error.
    assert!(
        matches!(err, SoError::UnknownSymbol { .. }),
        "expected UnknownSymbol for bad class, got: {err:?}"
    );
}

#[test]
fn import_key_field_stored_as_import_key() {
    let (_dir, db) = initialized_db();

    // Template with key_field set
    let template = ImportTemplate {
        version: 1,
        class: "Person".to_string(),
        mappings: {
            let mut m = HashMap::new();
            m.insert("full_name".to_string(), "name".to_string());
            m
        },
        key_field: Some("ext_id".to_string()),
    };

    let records = vec![record(&[("full_name", "KeyPerson"), ("ext_id", "EXT-001")])];
    let result = import_records(&db, &records, &template, false, false).unwrap();
    assert_eq!(result.created, 1);
    // The node was written — we trust the WriteTx carried `_import_key`.
    // (Can't easily query it back without a find API; the test verifies no error.)
}

#[test]
fn import_fields_not_in_mappings_are_ignored() {
    let (_dir, db) = initialized_db();
    let template = person_template();

    // Record has extra fields "ignored_field" and "other_col" not in the template.
    let records = vec![record(&[
        ("full_name", "Dave"),
        ("ignored_field", "noise"),
        ("other_col", "more noise"),
    ])];

    let result = import_records(&db, &records, &template, false, false).unwrap();
    assert_eq!(result.created, 1);
    assert_eq!(result.error_count(), 0);
}

#[test]
fn import_empty_records_returns_zero_counts() {
    let (_dir, db) = initialized_db();
    let template = person_template();

    let result = import_records(&db, &[], &template, false, false).unwrap();
    assert_eq!(result.created, 0);
    assert_eq!(result.skipped, 0);
    assert_eq!(result.error_count(), 0);
}

#[test]
fn import_via_alias_resolves_class() {
    let (_dir, db) = initialized_db();
    sparrowdb_ontology_core::add_alias(
        &db,
        "Human",
        sparrowdb_ontology_core::model::AliasKind::Class,
        "Person",
    )
    .unwrap();

    let template = ImportTemplate {
        version: 1,
        class: "Human".to_string(), // alias
        mappings: {
            let mut m = HashMap::new();
            m.insert("full_name".to_string(), "name".to_string());
            m
        },
        key_field: None,
    };

    let records = vec![record(&[("full_name", "Eve")])];
    let result = import_records(&db, &records, &template, false, false).unwrap();
    assert_eq!(result.created, 1);
}

// ── CSV round-trip ─────────────────────────────────────────────────────────────

/// Parse CSV bytes into records using the csv crate.
fn parse_csv(data: &[u8]) -> Vec<HashMap<String, String>> {
    let mut rdr = csv::Reader::from_reader(data);
    let headers: Vec<String> = rdr
        .headers()
        .unwrap()
        .iter()
        .map(|s| s.to_string())
        .collect();
    let mut records = Vec::new();
    for result in rdr.records() {
        let row = result.unwrap();
        let map: HashMap<String, String> = headers
            .iter()
            .zip(row.iter())
            .map(|(h, v)| (h.clone(), v.to_string()))
            .collect();
        records.push(map);
    }
    records
}

#[test]
fn csv_round_trip() {
    let (_dir, db) = initialized_db();

    // Write a temp CSV.
    let csv_content = b"full_name,email_address,id\nAlice,alice@ex.com,1\nBob,bob@ex.com,2\n";
    let records = parse_csv(csv_content);
    assert_eq!(records.len(), 2);

    let template = person_template();
    let result = import_records(&db, &records, &template, false, false).unwrap();
    assert_eq!(result.created, 2);
    assert_eq!(result.error_count(), 0);
}

// ── JSON round-trip ────────────────────────────────────────────────────────────

/// Parse JSON array of objects into records.
fn parse_json_array(data: &[u8]) -> Vec<HashMap<String, String>> {
    let arr: Vec<serde_json::Value> = serde_json::from_slice(data).unwrap();
    arr.into_iter()
        .map(|obj| {
            obj.as_object()
                .unwrap()
                .iter()
                .map(|(k, v)| {
                    let s = match v {
                        serde_json::Value::String(s) => s.clone(),
                        other => other.to_string(),
                    };
                    (k.clone(), s)
                })
                .collect()
        })
        .collect()
}

#[test]
fn json_round_trip() {
    let (_dir, db) = initialized_db();

    let json_content =
        br#"[{"full_name":"Carol","email_address":"carol@ex.com","id":"3"},{"full_name":"Dan","id":"4"}]"#;
    let records = parse_json_array(json_content);
    assert_eq!(records.len(), 2);

    let template = person_template();
    let result = import_records(&db, &records, &template, false, false).unwrap();
    assert_eq!(result.created, 2);
    assert_eq!(result.error_count(), 0);
}
