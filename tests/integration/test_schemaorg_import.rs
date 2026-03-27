// Integration tests for schema.org Turtle import — Tier 1 dataset.
//
// Verifies that schema.org-specific constructs (schema:Class, domainIncludes,
// rangeIncludes) are correctly mapped during Turtle import and round-trip
// through JSON-LD export.
//
// Structured for easy expansion: the `SCHEMAORG_SUBSET_TTL` constant can be
// grown toward full schema.org (900+ classes, 1400+ properties) by appending
// more definitions in the same format.

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

// ── Schema.org subset Turtle ───────────────────────────────────────────────────
//
// Minimal but representative: 4 classes with hierarchy, 5 properties covering
// single/multi domainIncludes and rangeIncludes. Uses the actual schema.org
// vocabulary patterns (schema:Class, schema:domainIncludes, schema:rangeIncludes).

const SCHEMAORG_SUBSET_TTL: &str = r#"
@prefix schema: <https://schema.org/> .
@prefix rdf:    <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .
@prefix rdfs:   <http://www.w3.org/2000/01/rdf-schema#> .
@prefix owl:    <http://www.w3.org/2002/07/owl#> .

# ── Classes ──────────────────────────────────────────────────────────────────

schema:Thing a schema:Class ;
    rdfs:label "Thing" ;
    rdfs:comment "The most generic type of item." .

schema:Person a schema:Class ;
    rdfs:label "Person" ;
    rdfs:comment "A person (alive, dead, undead, or fictional)." ;
    rdfs:subClassOf schema:Thing .

schema:Organization a schema:Class ;
    rdfs:label "Organization" ;
    rdfs:comment "An organization such as a school, NGO, corporation, club, etc." ;
    rdfs:subClassOf schema:Thing .

schema:LocalBusiness a schema:Class ;
    rdfs:label "LocalBusiness" ;
    rdfs:comment "A particular physical business or branch of an organization." ;
    rdfs:subClassOf schema:Organization .

# ── Properties with domainIncludes / rangeIncludes ───────────────────────────

# Single domainIncludes, no rangeIncludes (datatype-like)
schema:name a owl:DatatypeProperty ;
    rdfs:label "name" ;
    rdfs:comment "The name of the item." ;
    schema:domainIncludes schema:Thing .

# Single domainIncludes, single rangeIncludes (object-like)
schema:worksFor a owl:ObjectProperty ;
    rdfs:label "worksFor" ;
    rdfs:comment "Organizations that the person works for." ;
    schema:domainIncludes schema:Person ;
    schema:rangeIncludes schema:Organization .

# Multi domainIncludes → should drop domain with Unconstrained strategy
schema:email a owl:DatatypeProperty ;
    rdfs:label "email" ;
    rdfs:comment "Email address." ;
    schema:domainIncludes schema:Person ;
    schema:domainIncludes schema:Organization .

# Multi rangeIncludes → should drop range with Unconstrained strategy
schema:member a owl:ObjectProperty ;
    rdfs:label "member" ;
    rdfs:comment "A member of an Organization or a ProgramMembership." ;
    schema:domainIncludes schema:Organization ;
    schema:rangeIncludes schema:Organization ;
    schema:rangeIncludes schema:Person .

# Single domainIncludes, single rangeIncludes on subclass
schema:parentOrganization a owl:ObjectProperty ;
    rdfs:label "parentOrganization" ;
    rdfs:comment "The larger organization that this organization is a subOrganization of, if any." ;
    schema:domainIncludes schema:Organization ;
    schema:rangeIncludes schema:Organization .
"#;

// ══════════════════════════════════════════════════════════════════════════════
// Test 1 — Basic import counts
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn schemaorg_subset_import_counts() {
    let (_dir, db) = blank_db();
    let opts = ImportOptions {
        base_iri: None,
        domain_range_strategy: DomainRangeStrategy::Unconstrained,
    };
    let summary = import_turtle(&db, SCHEMAORG_SUBSET_TTL, opts).unwrap();

    assert_eq!(
        summary.classes_imported, 4,
        "expected 4 classes (Thing, Person, Organization, LocalBusiness), got {}",
        summary.classes_imported
    );
    assert_eq!(
        summary.relations_imported, 5,
        "expected 5 relations (name, worksFor, email, member, parentOrganization), got {}",
        summary.relations_imported
    );
    assert_eq!(
        summary.subclasses_imported, 3,
        "expected 3 subclass pairs (Person→Thing, Organization→Thing, LocalBusiness→Organization), got {}",
        summary.subclasses_imported
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// Test 2 — schema:Class recognised as class type
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn schema_class_type_recognised() {
    let (_dir, db) = blank_db();
    let opts = ImportOptions {
        base_iri: None,
        domain_range_strategy: DomainRangeStrategy::Unconstrained,
    };
    import_turtle(&db, SCHEMAORG_SUBSET_TTL, opts).unwrap();

    let doc = export_json_ld(&db).unwrap();
    let graph = get_graph(&doc);

    // All four classes must appear in the export
    for label in &["Thing", "Person", "Organization", "LocalBusiness"] {
        assert!(
            find_node_by_label(graph, label).is_some(),
            "class '{label}' must be present in JSON-LD export"
        );
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// Test 3 — IRIs stored correctly for schema.org entities
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn schemaorg_iris_stored() {
    let (_dir, db) = blank_db();
    let opts = ImportOptions {
        base_iri: None,
        domain_range_strategy: DomainRangeStrategy::Unconstrained,
    };
    import_turtle(&db, SCHEMAORG_SUBSET_TTL, opts).unwrap();

    let doc = export_json_ld(&db).unwrap();
    let graph = get_graph(&doc);

    let person = find_node_by_label(graph, "Person").expect("Person not found");
    assert_eq!(
        person["@id"].as_str(),
        Some("https://schema.org/Person"),
        "Person @id must be full schema.org IRI"
    );

    let org = find_node_by_label(graph, "Organization").expect("Organization not found");
    assert_eq!(
        org["@id"].as_str(),
        Some("https://schema.org/Organization"),
        "Organization @id must be full schema.org IRI"
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// Test 4 — Single domainIncludes resolved to domain
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn single_domain_includes_resolved() {
    let (_dir, db) = blank_db();
    let opts = ImportOptions {
        base_iri: None,
        domain_range_strategy: DomainRangeStrategy::Unconstrained,
    };
    import_turtle(&db, SCHEMAORG_SUBSET_TTL, opts).unwrap();

    let doc = export_json_ld(&db).unwrap();
    let graph = get_graph(&doc);

    // schema:name has single domainIncludes (Thing) → domain should be set
    let name_node = find_node_by_label(graph, "name").expect("'name' relation not found");
    assert!(
        name_node.get("rdfs:domain").is_some(),
        "rdfs:domain must be present for 'name' (single domainIncludes → Thing)"
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// Test 5 — Multi domainIncludes drops domain (Unconstrained strategy)
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn multi_domain_includes_drops_domain_unconstrained() {
    let (_dir, db) = blank_db();
    let opts = ImportOptions {
        base_iri: None,
        domain_range_strategy: DomainRangeStrategy::Unconstrained,
    };
    import_turtle(&db, SCHEMAORG_SUBSET_TTL, opts).unwrap();

    let doc = export_json_ld(&db).unwrap();
    let graph = get_graph(&doc);

    // schema:email has 2 domainIncludes (Person, Organization) → domain absent
    let email_node = find_node_by_label(graph, "email").expect("'email' relation not found");
    assert!(
        email_node.get("rdfs:domain").is_none(),
        "rdfs:domain must be absent for 'email' (multiple domainIncludes with Unconstrained)"
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// Test 6 — Multi rangeIncludes drops range (Unconstrained strategy)
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn multi_range_includes_drops_range_unconstrained() {
    let (_dir, db) = blank_db();
    let opts = ImportOptions {
        base_iri: None,
        domain_range_strategy: DomainRangeStrategy::Unconstrained,
    };
    import_turtle(&db, SCHEMAORG_SUBSET_TTL, opts).unwrap();

    let doc = export_json_ld(&db).unwrap();
    let graph = get_graph(&doc);

    // schema:member has 2 rangeIncludes (Organization, Person) → range absent
    let member_node = find_node_by_label(graph, "member").expect("'member' relation not found");
    assert!(
        member_node.get("rdfs:range").is_none(),
        "rdfs:range must be absent for 'member' (multiple rangeIncludes with Unconstrained)"
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// Test 7 — Single rangeIncludes resolved to range
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn single_range_includes_resolved() {
    let (_dir, db) = blank_db();
    let opts = ImportOptions {
        base_iri: None,
        domain_range_strategy: DomainRangeStrategy::Unconstrained,
    };
    import_turtle(&db, SCHEMAORG_SUBSET_TTL, opts).unwrap();

    let doc = export_json_ld(&db).unwrap();
    let graph = get_graph(&doc);

    // schema:worksFor has single rangeIncludes (Organization) → range set
    let wf_node = find_node_by_label(graph, "worksFor").expect("'worksFor' relation not found");
    assert!(
        wf_node.get("rdfs:range").is_some(),
        "rdfs:range must be present for 'worksFor' (single rangeIncludes → Organization)"
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// Test 8 — Subclass hierarchy preserved in JSON-LD
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn subclass_hierarchy_in_export() {
    let (_dir, db) = blank_db();
    let opts = ImportOptions {
        base_iri: None,
        domain_range_strategy: DomainRangeStrategy::Unconstrained,
    };
    import_turtle(&db, SCHEMAORG_SUBSET_TTL, opts).unwrap();

    let doc = export_json_ld(&db).unwrap();
    let graph = get_graph(&doc);

    // Person → subClassOf Thing (verify link exists and references Thing)
    let person = find_node_by_label(graph, "Person").expect("Person not found");
    let person_subclass = person
        .get("rdfs:subClassOf")
        .expect("Person must have rdfs:subClassOf in export");
    let person_subclass_str = serde_json::to_string(person_subclass).unwrap();
    assert!(
        person_subclass_str.contains("Thing"),
        "Person's subClassOf should reference Thing, got: {}",
        person_subclass_str
    );

    // LocalBusiness → subClassOf Organization
    let lb = find_node_by_label(graph, "LocalBusiness").expect("LocalBusiness not found");
    let lb_subclass = lb
        .get("rdfs:subClassOf")
        .expect("LocalBusiness must have rdfs:subClassOf in export");
    let lb_subclass_str = serde_json::to_string(lb_subclass).unwrap();
    assert!(
        lb_subclass_str.contains("Organization"),
        "LocalBusiness's subClassOf should reference Organization, got: {}",
        lb_subclass_str
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// Test 9 — FirstOnly strategy keeps first domain even when multiple exist
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn first_only_strategy_keeps_domain() {
    let (_dir, db) = blank_db();
    let opts = ImportOptions {
        base_iri: None,
        domain_range_strategy: DomainRangeStrategy::FirstOnly,
    };
    import_turtle(&db, SCHEMAORG_SUBSET_TTL, opts).unwrap();

    let doc = export_json_ld(&db).unwrap();
    let graph = get_graph(&doc);

    // With FirstOnly, email (multi domainIncludes) should still have a domain
    let email_node = find_node_by_label(graph, "email").expect("'email' relation not found");
    assert!(
        email_node.get("rdfs:domain").is_some(),
        "rdfs:domain must be present for 'email' with FirstOnly strategy"
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// Test 10 — Round-trip: import subset then export, no data loss
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn round_trip_no_data_loss() {
    let (_dir, db) = blank_db();
    let opts = ImportOptions {
        base_iri: None,
        domain_range_strategy: DomainRangeStrategy::Unconstrained,
    };
    import_turtle(&db, SCHEMAORG_SUBSET_TTL, opts).unwrap();

    let doc = export_json_ld(&db).unwrap();
    let graph = get_graph(&doc);

    // Count classes (type = owl:Class or rdfs:Class) and relations
    let class_labels = ["Thing", "Person", "Organization", "LocalBusiness"];
    let relation_labels = ["name", "worksFor", "email", "member", "parentOrganization"];

    for label in &class_labels {
        assert!(
            find_node_by_label(graph, label).is_some(),
            "class '{label}' missing from JSON-LD round-trip"
        );
    }

    for label in &relation_labels {
        assert!(
            find_node_by_label(graph, label).is_some(),
            "relation '{label}' missing from JSON-LD round-trip"
        );
    }

    // Verify descriptions survived
    let thing = find_node_by_label(graph, "Thing").expect("Thing not found");
    assert_eq!(
        thing["rdfs:comment"].as_str(),
        Some("The most generic type of item."),
        "Thing description must survive round-trip"
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// Test 11 — No warnings on clean schema.org subset import
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn clean_import_no_warnings() {
    let (_dir, db) = blank_db();
    let opts = ImportOptions {
        base_iri: None,
        domain_range_strategy: DomainRangeStrategy::Unconstrained,
    };
    let summary = import_turtle(&db, SCHEMAORG_SUBSET_TTL, opts).unwrap();

    assert!(
        summary.warnings.is_empty(),
        "clean schema.org subset import should produce no warnings, got: {:?}",
        summary.warnings
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// Test 12 — Idempotent re-import of schema.org subset
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn idempotent_reimport_schema_org() {
    let (_dir, db) = blank_db();
    let opts = ImportOptions {
        base_iri: None,
        domain_range_strategy: DomainRangeStrategy::Unconstrained,
    };

    // First import
    let s1 = import_turtle(&db, SCHEMAORG_SUBSET_TTL, opts.clone()).unwrap();
    assert_eq!(s1.classes_imported, 4);

    // Second import — must not error
    let s2 = import_turtle(&db, SCHEMAORG_SUBSET_TTL, opts).unwrap();
    assert!(
        s2.warnings
            .iter()
            .all(|w| !w.to_lowercase().contains("error")),
        "re-import must not produce error-level warnings, got: {:?}",
        s2.warnings
    );

    // Verify the import summary is consistent (should report same items as first)
    // Note: This validates idempotency — re-importing same data should recognize and reuse existing nodes
    assert_eq!(
        s1.classes_imported, s2.classes_imported,
        "re-importing same data should detect and reuse existing classes"
    );
}
