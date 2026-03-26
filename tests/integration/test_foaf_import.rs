// Integration tests for FOAF ontology import — Tier 1 real-world dataset.
//
// Uses a representative subset of the real FOAF (Friend of a Friend) vocabulary
// to verify: happy-path import, JSON-LD round-trip, idempotency, and IRI
// preservation.

use sparrowdb::GraphDb;
use sparrowdb_ontology_core::{
    export_json_ld, init,
    turtle_import::{import_turtle, DomainRangeStrategy, ImportOptions},
    StarterKind,
};

// ── DB helpers ─────────────────────────────────────────────────────────────────

fn blank_db() -> (tempfile::TempDir, GraphDb) {
    let dir = tempfile::tempdir().unwrap();
    let db = GraphDb::open(dir.path()).unwrap();
    init(&db, Some(StarterKind::Blank), false).unwrap();
    (dir, db)
}

// ── JSON-LD helpers ────────────────────────────────────────────────────────────

fn get_graph(doc: &serde_json::Value) -> &Vec<serde_json::Value> {
    doc["@graph"].as_array().expect("@graph must be an array")
}

fn find_node_by_label<'a>(
    graph: &'a [serde_json::Value],
    label: &str,
) -> Option<&'a serde_json::Value> {
    graph
        .iter()
        .find(|node| node["rdfs:label"].as_str() == Some(label))
}

// ── Real FOAF vocabulary subset ────────────────────────────────────────────────
//
// This is a representative extract from the actual FOAF spec at
// http://xmlns.com/foaf/0.1/.  It includes classes, object properties,
// datatype properties, subclass relationships, domain/range declarations,
// and comments — exercising all the main import code paths.

const FOAF_REAL_TTL: &str = r#"
@prefix rdf:  <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
@prefix owl:  <http://www.w3.org/2002/07/owl#> .
@prefix foaf: <http://xmlns.com/foaf/0.1/> .

# ── Classes ──────────────────────────────────────────────────────────

foaf:Agent a owl:Class ;
    rdfs:label "Agent" ;
    rdfs:comment "An agent (eg. person, group, software or physical artifact)." .

foaf:Person a owl:Class ;
    rdfs:label "Person" ;
    rdfs:comment "A person." ;
    rdfs:subClassOf foaf:Agent .

foaf:Organization a owl:Class ;
    rdfs:label "Organization" ;
    rdfs:comment "An organization." ;
    rdfs:subClassOf foaf:Agent .

foaf:Group a owl:Class ;
    rdfs:label "Group" ;
    rdfs:comment "A class of Agents." ;
    rdfs:subClassOf foaf:Agent .

foaf:Document a owl:Class ;
    rdfs:label "Document" ;
    rdfs:comment "A document." .

foaf:Image a owl:Class ;
    rdfs:label "Image" ;
    rdfs:comment "An image." ;
    rdfs:subClassOf foaf:Document .

foaf:OnlineAccount a owl:Class ;
    rdfs:label "Online Account" ;
    rdfs:comment "An online account." .

foaf:Project a owl:Class ;
    rdfs:label "Project" ;
    rdfs:comment "A project (a collective endeavour of some kind)." .

# ── Object Properties ────────────────────────────────────────────────

foaf:knows a owl:ObjectProperty ;
    rdfs:label "knows" ;
    rdfs:comment "A person known by this person (indicating some level of reciprocated interaction between the parties)." ;
    rdfs:domain foaf:Person ;
    rdfs:range foaf:Person .

foaf:member a owl:ObjectProperty ;
    rdfs:label "member" ;
    rdfs:comment "Indicates a member of a Group." ;
    rdfs:domain foaf:Group ;
    rdfs:range foaf:Agent .

foaf:depiction a owl:ObjectProperty ;
    rdfs:label "depiction" ;
    rdfs:comment "A depiction of some thing." ;
    rdfs:range foaf:Image .

foaf:account a owl:ObjectProperty ;
    rdfs:label "account" ;
    rdfs:comment "Indicates an account held by this agent." ;
    rdfs:domain foaf:Agent ;
    rdfs:range foaf:OnlineAccount .

foaf:currentProject a owl:ObjectProperty ;
    rdfs:label "currentProject" ;
    rdfs:comment "A current project this person works on." ;
    rdfs:domain foaf:Person ;
    rdfs:range foaf:Project .

# ── Datatype Properties ──────────────────────────────────────────────

foaf:name a owl:DatatypeProperty ;
    rdfs:label "name" ;
    rdfs:comment "A name for some thing." .

foaf:mbox a owl:DatatypeProperty ;
    rdfs:label "mbox" ;
    rdfs:comment "A personal mailbox, ie. an Internet mailbox associated with exactly one owner." ;
    rdfs:domain foaf:Agent .

foaf:homepage a owl:DatatypeProperty ;
    rdfs:label "homepage" ;
    rdfs:comment "A homepage for some thing." .
"#;

// ══════════════════════════════════════════════════════════════════════════════
// Test 1 — Happy path: correct counts for classes, relations, subclasses
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn foaf_real_happy_path_counts() {
    let (_dir, db) = blank_db();
    let summary = import_turtle(&db, FOAF_REAL_TTL, ImportOptions::default()).unwrap();

    assert_eq!(
        summary.classes_imported, 8,
        "expected 8 classes (Agent, Person, Organization, Group, Document, Image, OnlineAccount, Project), got {}",
        summary.classes_imported
    );

    // 5 object properties + 3 datatype properties = 8 relations
    assert_eq!(
        summary.relations_imported, 8,
        "expected 8 relations (knows, member, depiction, account, currentProject, name, mbox, homepage), got {}",
        summary.relations_imported
    );

    // Person→Agent, Organization→Agent, Group→Agent, Image→Document = 4
    assert_eq!(
        summary.subclasses_imported, 4,
        "expected 4 subclass pairs, got {}",
        summary.subclasses_imported
    );

    // Filter out the blank-node skip warning if present — no real warnings expected
    let real_warnings: Vec<_> = summary
        .warnings
        .iter()
        .filter(|w| !w.contains("blank-node"))
        .collect();
    assert!(
        real_warnings.is_empty(),
        "expected no real warnings, got: {:?}",
        real_warnings
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// Test 2 — JSON-LD round-trip: import FOAF → export → verify key nodes exist
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn foaf_jsonld_round_trip() {
    let (_dir, db) = blank_db();
    import_turtle(&db, FOAF_REAL_TTL, ImportOptions::default()).unwrap();

    let doc = export_json_ld(&db).unwrap();
    let graph = get_graph(&doc);

    // Verify core classes are present
    for class_label in &[
        "Person",
        "Organization",
        "Agent",
        "Group",
        "Document",
        "Image",
        "Online Account",
        "Project",
    ] {
        assert!(
            find_node_by_label(graph, class_label).is_some(),
            "class '{}' must be present in JSON-LD export",
            class_label
        );
    }

    // Verify core relations are present
    for rel_label in &["knows", "member", "name", "mbox", "account"] {
        assert!(
            find_node_by_label(graph, rel_label).is_some(),
            "relation '{}' must be present in JSON-LD export",
            rel_label
        );
    }

    // Verify Person has subClassOf
    let person = find_node_by_label(graph, "Person").unwrap();
    assert!(
        person.get("rdfs:subClassOf").is_some(),
        "Person must have rdfs:subClassOf (Agent) in JSON-LD export"
    );

    // Verify Image has subClassOf Document
    let image = find_node_by_label(graph, "Image").unwrap();
    assert!(
        image.get("rdfs:subClassOf").is_some(),
        "Image must have rdfs:subClassOf (Document) in JSON-LD export"
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// Test 3 — IRI preservation: all FOAF IRIs use http://xmlns.com/foaf/0.1/
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn foaf_iri_preservation() {
    let (_dir, db) = blank_db();
    import_turtle(&db, FOAF_REAL_TTL, ImportOptions::default()).unwrap();

    let doc = export_json_ld(&db).unwrap();
    let graph = get_graph(&doc);

    let expected_iris = vec![
        ("Person", "http://xmlns.com/foaf/0.1/Person"),
        ("Organization", "http://xmlns.com/foaf/0.1/Organization"),
        ("Agent", "http://xmlns.com/foaf/0.1/Agent"),
        ("Document", "http://xmlns.com/foaf/0.1/Document"),
        ("Image", "http://xmlns.com/foaf/0.1/Image"),
    ];

    for (label, expected_iri) in &expected_iris {
        let node = find_node_by_label(graph, label)
            .unwrap_or_else(|| panic!("node '{}' not found in @graph", label));
        assert_eq!(
            node["@id"].as_str(),
            Some(*expected_iri),
            "@id for '{}' must be the full FOAF IRI",
            label
        );
    }

    // Also verify a relation IRI
    let knows = find_node_by_label(graph, "knows").expect("'knows' relation not found");
    assert_eq!(
        knows["@id"].as_str(),
        Some("http://xmlns.com/foaf/0.1/knows"),
        "@id for 'knows' must be the full FOAF IRI"
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// Test 4 — Idempotency: importing the same FOAF twice does not error
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn foaf_idempotent_reimport() {
    let (_dir, db) = blank_db();

    // First import
    let s1 = import_turtle(&db, FOAF_REAL_TTL, ImportOptions::default()).unwrap();
    assert_eq!(s1.classes_imported, 8, "first import: expected 8 classes");
    assert_eq!(s1.relations_imported, 8, "first import: expected 8 relations");

    // Second import — must succeed without error
    let s2 = import_turtle(&db, FOAF_REAL_TTL, ImportOptions::default());
    assert!(
        s2.is_ok(),
        "second import of identical FOAF must return Ok, got: {:?}",
        s2.err()
    );

    let s2 = s2.unwrap();
    // No error-level warnings
    assert!(
        s2.warnings
            .iter()
            .all(|w| !w.to_lowercase().contains("error")),
        "second import must not produce error-level warnings, got: {:?}",
        s2.warnings
    );

    // After two imports, JSON-LD export should still produce valid output
    let doc = export_json_ld(&db).unwrap();
    let graph = get_graph(&doc);
    assert!(
        find_node_by_label(graph, "Person").is_some(),
        "Person must exist after double import"
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// Test 5 — Domain/range edges on 'knows' relation (Person→Person)
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn foaf_knows_domain_range() {
    let (_dir, db) = blank_db();
    import_turtle(&db, FOAF_REAL_TTL, ImportOptions::default()).unwrap();

    let doc = export_json_ld(&db).unwrap();
    let graph = get_graph(&doc);

    let knows = find_node_by_label(graph, "knows").expect("'knows' not found in @graph");

    // knows has domain Person and range Person
    assert!(
        knows.get("rdfs:domain").is_some(),
        "'knows' must have rdfs:domain in JSON-LD export"
    );
    assert!(
        knows.get("rdfs:range").is_some(),
        "'knows' must have rdfs:range in JSON-LD export"
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// Test 6 — Comments/descriptions are preserved
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn foaf_descriptions_preserved() {
    let (_dir, db) = blank_db();
    import_turtle(&db, FOAF_REAL_TTL, ImportOptions::default()).unwrap();

    let doc = export_json_ld(&db).unwrap();
    let graph = get_graph(&doc);

    let person = find_node_by_label(graph, "Person").expect("Person not found");
    let desc = person["rdfs:comment"].as_str().unwrap_or("");
    assert_eq!(desc, "A person.", "Person description must be preserved");

    let agent = find_node_by_label(graph, "Agent").expect("Agent not found");
    let desc = agent["rdfs:comment"].as_str().unwrap_or("");
    assert!(
        desc.contains("agent"),
        "Agent description must contain 'agent', got: {}",
        desc
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// Test 7 — FirstOnly strategy works with FOAF (single domain/range per prop)
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn foaf_first_only_strategy() {
    let (_dir, db) = blank_db();

    let opts = ImportOptions {
        base_iri: None,
        domain_range_strategy: DomainRangeStrategy::FirstOnly,
    };
    let summary = import_turtle(&db, FOAF_REAL_TTL, opts).unwrap();

    // Same counts — strategy only affects multi-domain handling
    assert_eq!(summary.classes_imported, 8);
    assert_eq!(summary.relations_imported, 8);

    // 'knows' should still have domain/range with FirstOnly
    let doc = export_json_ld(&db).unwrap();
    let graph = get_graph(&doc);
    let knows = find_node_by_label(graph, "knows").expect("'knows' not found");
    assert!(
        knows.get("rdfs:domain").is_some(),
        "'knows' must have rdfs:domain with FirstOnly strategy"
    );
}
