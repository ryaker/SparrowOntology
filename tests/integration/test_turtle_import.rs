// Integration tests for `import_turtle` — GitHub issue #30.
//
// Covers: FOAF happy path, IRI storage, schema.org domainIncludes,
// subclass import, cyclic subclass graceful-fail, blank nodes, idempotency,
// skos:altLabel aliases, OWL Families Primer extract, language preference,
// and empty input.

use sparrowdb::GraphDb;
use sparrowdb_ontology_core::{
    export_json_ld, init,
    turtle_import::{import_turtle, DomainRangeStrategy, ImportOptions},
    StarterKind,
};

// ── DB helpers ─────────────────────────────────────────────────────────────────

/// Open a blank-initialised DB (no starter classes or relations).
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

// ── FOAF snippet ───────────────────────────────────────────────────────────────

const FOAF_TTL: &str = r#"
@prefix owl: <http://www.w3.org/2002/07/owl#> .
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
@prefix foaf: <http://xmlns.com/foaf/0.1/> .

foaf:Person a owl:Class ;
    rdfs:label "Person" ;
    rdfs:comment "A person." .

foaf:Organization a owl:Class ;
    rdfs:label "Organization" ;
    rdfs:comment "An organization." .

foaf:knows a owl:ObjectProperty ;
    rdfs:label "knows" ;
    rdfs:domain foaf:Person ;
    rdfs:range foaf:Person .
"#;

// ══════════════════════════════════════════════════════════════════════════════
// Test 1 — FOAF happy path
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn foaf_happy_path_counts() {
    let (_dir, db) = blank_db();
    let summary = import_turtle(&db, FOAF_TTL, ImportOptions::default()).unwrap();

    assert_eq!(
        summary.classes_imported, 2,
        "expected 2 classes (Person, Organization), got {}",
        summary.classes_imported
    );
    assert_eq!(
        summary.relations_imported, 1,
        "expected 1 relation (knows), got {}",
        summary.relations_imported
    );
    assert!(
        summary.warnings.is_empty(),
        "expected no warnings, got: {:?}",
        summary.warnings
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// Test 2 — IRI is stored on imported class
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn iri_stored_on_imported_class() {
    let (_dir, db) = blank_db();

    let ttl = r#"
@prefix owl: <http://www.w3.org/2002/07/owl#> .
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .

<https://schema.org/Organization> a owl:Class ;
    rdfs:label "Organization" .
"#;

    import_turtle(&db, ttl, ImportOptions::default()).unwrap();

    let doc = export_json_ld(&db).unwrap();
    let graph = get_graph(&doc);
    let node = find_node_by_label(graph, "Organization").expect("Organization node not found");

    assert_eq!(
        node["@id"].as_str(),
        Some("https://schema.org/Organization"),
        "@id must be the full IRI when one is present in the Turtle source"
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// Test 3 — schema.org domainIncludes multi-value → Unconstrained (no domain)
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn schema_domain_includes_multi_value_unconstrained() {
    let (_dir, db) = blank_db();

    // NOTE: schema:domainIncludes uses rdfs:label on `schema:name` so the
    // relation gets a usable name.  `schema:Person` and `schema:Organization`
    // use rdf:type schema:Class.
    let ttl = r#"
@prefix schema: <https://schema.org/> .
@prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
@prefix owl: <http://www.w3.org/2002/07/owl#> .

schema:Person a schema:Class ;
    rdfs:label "Person" .
schema:Organization a schema:Class ;
    rdfs:label "Organization" .
schema:name a owl:DatatypeProperty ;
    rdfs:label "name" ;
    schema:domainIncludes schema:Person ;
    schema:domainIncludes schema:Organization .
"#;

    let opts = ImportOptions {
        base_iri: None,
        domain_range_strategy: DomainRangeStrategy::Unconstrained,
    };
    let summary = import_turtle(&db, ttl, opts).unwrap();

    assert_eq!(
        summary.relations_imported, 1,
        "expected 1 relation (name), got {}",
        summary.relations_imported
    );

    // With Unconstrained strategy and 2 domains, no rdfs:domain edge is created.
    let doc = export_json_ld(&db).unwrap();
    let graph = get_graph(&doc);
    let rel_node = find_node_by_label(graph, "name").expect("'name' relation not found in @graph");

    assert!(
        rel_node.get("rdfs:domain").is_none(),
        "rdfs:domain must be absent when multiple domainIncludes values exist with Unconstrained strategy"
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// Test 4 — schema.org domainIncludes single-value → resolved domain
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn schema_domain_includes_single_value_resolved() {
    let (_dir, db) = blank_db();

    let ttl = r#"
@prefix schema: <https://schema.org/> .
@prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
@prefix owl: <http://www.w3.org/2002/07/owl#> .

schema:Person a schema:Class ;
    rdfs:label "Person" .
schema:email a owl:DatatypeProperty ;
    rdfs:label "email" ;
    schema:domainIncludes schema:Person .
"#;

    let opts = ImportOptions {
        base_iri: None,
        domain_range_strategy: DomainRangeStrategy::Unconstrained,
    };
    import_turtle(&db, ttl, opts).unwrap();

    let doc = export_json_ld(&db).unwrap();
    let graph = get_graph(&doc);
    let rel_node =
        find_node_by_label(graph, "email").expect("'email' relation not found in @graph");

    // Single domainIncludes with Unconstrained → domain is set
    assert!(
        rel_node.get("rdfs:domain").is_some(),
        "rdfs:domain must be present when exactly one domainIncludes value exists"
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// Test 5 — Subclass relationship imported
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn subclass_relationship_imported() {
    let (_dir, db) = blank_db();

    let ttl = r#"
@prefix owl: <http://www.w3.org/2002/07/owl#> .
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .

<https://example.org/Animal> a owl:Class ; rdfs:label "Animal" .
<https://example.org/Dog> a owl:Class ; rdfs:label "Dog" ;
    rdfs:subClassOf <https://example.org/Animal> .
"#;

    let summary = import_turtle(&db, ttl, ImportOptions::default()).unwrap();

    assert_eq!(
        summary.subclasses_imported, 1,
        "expected 1 subclass pair (Dog → Animal), got {}",
        summary.subclasses_imported
    );

    // Verify the exported JSON-LD includes rdfs:subClassOf on Dog.
    let doc = export_json_ld(&db).unwrap();
    let graph = get_graph(&doc);
    let dog_node = find_node_by_label(graph, "Dog").expect("Dog node not found");

    assert!(
        dog_node.get("rdfs:subClassOf").is_some(),
        "Dog must have rdfs:subClassOf in JSON-LD export"
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// Test 6 — Cyclic subclass → warning, not error
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn cyclic_subclass_returns_ok_with_warning() {
    let (_dir, db) = blank_db();

    // A and A2 both have label "A".  B (subClassOf A).  A2 subClassOf B.
    // When define_subclass("A", "B") is attempted (A2's label = "A"),
    // that creates a cycle A→B→A which the implementation converts to a warning.
    let ttl = r#"
@prefix owl: <http://www.w3.org/2002/07/owl#> .
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .

<https://example.org/A> a owl:Class ; rdfs:label "A" .
<https://example.org/B> a owl:Class ; rdfs:label "B" ;
    rdfs:subClassOf <https://example.org/A> .
<https://example.org/A2> a owl:Class ; rdfs:label "A" ;
    rdfs:subClassOf <https://example.org/B> .
"#;

    let result = import_turtle(&db, ttl, ImportOptions::default());
    assert!(
        result.is_ok(),
        "import_turtle must return Ok even when a cycle is detected"
    );

    let summary = result.unwrap();
    assert!(
        !summary.warnings.is_empty(),
        "expected at least one warning for the cycle, got none"
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// Test 7 — Unsupported OWL constructs (blank-node subjects) silently skipped
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn unsupported_owl_restriction_blank_node_skipped() {
    let (_dir, db) = blank_db();

    let ttl = r#"
@prefix owl: <http://www.w3.org/2002/07/owl#> .
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .

<https://example.org/Person> a owl:Class ; rdfs:label "Person" .

[ a owl:Restriction ;
  owl:onProperty <https://example.org/age> ;
  owl:minCardinality 1 ] .
"#;

    let result = import_turtle(&db, ttl, ImportOptions::default());
    assert!(
        result.is_ok(),
        "import_turtle must not error on blank-node restrictions"
    );

    let summary = result.unwrap();
    assert_eq!(
        summary.classes_imported, 1,
        "only Person (named node) should be imported; blank-node restriction skipped"
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// Test 8 — Idempotent re-import (both calls return Ok, no errors)
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn idempotent_reimport_both_calls_return_ok() {
    let (_dir, db) = blank_db();

    // First import
    let s1 = import_turtle(&db, FOAF_TTL, ImportOptions::default());
    assert!(s1.is_ok(), "first import must return Ok");
    let s1 = s1.unwrap();
    assert_eq!(s1.classes_imported, 2, "first import: expected 2 classes");
    assert_eq!(
        s1.relations_imported, 1,
        "first import: expected 1 relation"
    );

    // Second import of the same Turtle — must also return Ok (no error)
    let s2 = import_turtle(&db, FOAF_TTL, ImportOptions::default());
    assert!(
        s2.is_ok(),
        "second import of identical Turtle must also return Ok"
    );

    let s2 = s2.unwrap();
    // Each call reports what it wrote in that pass — at minimum no errors.
    assert!(
        s2.warnings
            .iter()
            .all(|w| !w.to_lowercase().contains("error")),
        "second import must not produce error-level warnings, got: {:?}",
        s2.warnings
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// Test 9 — skos:altLabel imported as aliases
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn skos_alt_label_imported_as_aliases() {
    let (_dir, db) = blank_db();

    let ttl = r#"
@prefix owl: <http://www.w3.org/2002/07/owl#> .
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
@prefix skos: <http://www.w3.org/2004/02/skos/core#> .

<https://example.org/Person> a owl:Class ;
    rdfs:label "Person" ;
    skos:altLabel "Human" ;
    skos:altLabel "Individual" .
"#;

    let summary = import_turtle(&db, ttl, ImportOptions::default()).unwrap();

    assert_eq!(
        summary.aliases_imported, 2,
        "expected 2 aliases (Human, Individual), got {}",
        summary.aliases_imported
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// Test 10 — OWL Families Primer minimal extract
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn owl_families_primer_valid_constructs_imported() {
    let (_dir, db) = blank_db();

    let ttl = r#"
@prefix owl: <http://www.w3.org/2002/07/owl#> .
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .

<http://example.org/family#Person> a owl:Class ; rdfs:label "Person" .
<http://example.org/family#Woman> a owl:Class ; rdfs:label "Woman" ;
    rdfs:subClassOf <http://example.org/family#Person> .
<http://example.org/family#hasSpouse> a owl:ObjectProperty ;
    rdfs:label "hasSpouse" ;
    rdfs:domain <http://example.org/family#Person> ;
    rdfs:range <http://example.org/family#Person> .
<http://example.org/family#Bill> a <http://example.org/family#Person> .
"#;

    let summary = import_turtle(&db, ttl, ImportOptions::default()).unwrap();

    assert_eq!(
        summary.classes_imported, 2,
        "expected 2 classes (Person, Woman), got {}",
        summary.classes_imported
    );
    assert_eq!(
        summary.relations_imported, 1,
        "expected 1 relation (hasSpouse), got {}",
        summary.relations_imported
    );
    assert_eq!(
        summary.subclasses_imported, 1,
        "expected 1 subclass pair (Woman → Person), got {}",
        summary.subclasses_imported
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// Test 11 — English label preference over untagged / other languages
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn english_label_preferred_over_other_languages() {
    let (_dir, db) = blank_db();

    let ttl = r#"
@prefix owl: <http://www.w3.org/2002/07/owl#> .
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .

<https://example.org/Person> a owl:Class ;
    rdfs:label "Personne"@fr ;
    rdfs:label "Person"@en ;
    rdfs:label "Persona"@es .
"#;

    let summary = import_turtle(&db, ttl, ImportOptions::default()).unwrap();
    assert_eq!(
        summary.classes_imported, 1,
        "expected 1 class, got {}",
        summary.classes_imported
    );

    // The English label must win.
    let doc = export_json_ld(&db).unwrap();
    let graph = get_graph(&doc);
    let node = find_node_by_label(graph, "Person")
        .expect("class 'Person' (English label) not found in @graph");
    assert_eq!(
        node["rdfs:label"].as_str(),
        Some("Person"),
        "English label 'Person' must be stored, not the French or Spanish alternative"
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// Test 12 — Empty Turtle string → empty summary
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn empty_turtle_returns_empty_summary() {
    let (_dir, db) = blank_db();
    let summary = import_turtle(&db, "", ImportOptions::default()).unwrap();

    assert_eq!(
        summary.classes_imported, 0,
        "empty input: classes_imported must be 0"
    );
    assert_eq!(
        summary.relations_imported, 0,
        "empty input: relations_imported must be 0"
    );
    assert_eq!(
        summary.subclasses_imported, 0,
        "empty input: subclasses_imported must be 0"
    );
    assert_eq!(
        summary.aliases_imported, 0,
        "empty input: aliases_imported must be 0"
    );
    assert!(
        summary.warnings.is_empty(),
        "empty input: warnings must be empty"
    );
}
