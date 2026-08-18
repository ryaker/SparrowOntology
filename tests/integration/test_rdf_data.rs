//! WS1 end-to-end tests: instance-data RDF export / import.
//!
//! These hit a real `GraphDb` on real disk and include a `checkpoint()` step —
//! a class of bug in this engine only shows up after a checkpoint, so a test
//! that never checkpoints does not prove the feature works.
//!
//! **Every expected value below is derived by hand from the fixture, with the
//! derivation written out in a comment.** Nothing here was captured from
//! program output. Capturing output into assertions is how real bugs get
//! frozen into a test suite as "expected behaviour".

use std::collections::{HashMap, HashSet};
use std::path::Path;

use sparrowdb::GraphDb;
use sparrowdb_common::NodeId;
use sparrowdb_ontology_core::init::{add_property, init, StarterKind};
use sparrowdb_ontology_core::rdf_data::{
    export_data_json_ld, export_data_turtle, export_data_with_report, import_data_turtle,
    ImportStrategy,
};
use sparrowdb_storage::node_store::Value as StoreValue;

const BASE: &str = "https://example.org/kb";

// ── Fixture ───────────────────────────────────────────────────────────────────
//
// Deliberately MULTI-LABEL and MULTI-RELATION. A single-label fixture cannot
// reach the bugs this engine produces — node ids are label-scoped
// (`NodeId = (label_id << 32) | slot`), so Person slot 0 and Organization
// slot 0 are different nodes with ids 0 and 4294967296. Only a fixture with
// several labels can catch code that confuses a slot for an identity.
//
// Classes used:        Person, Organization, Project        (3  — spec bar: >=3)
// Entities:            5 + 3 + 3 = 11                       (11 — spec bar: >=10)
// Relationships:       4 + 4 + 2 = 10                       (10 — spec bar: >=8)
// Relation types:      WORKS_FOR, KNOWS, OWNS               (3  — spec bar: >=2)

fn sv(s: &str) -> StoreValue {
    StoreValue::Bytes(s.as_bytes().to_vec())
}

/// Declare the ontology: the WorldModel starter plus four extra Project
/// properties chosen to cover every branch of the datatype mapping table
/// (float64, int64, bool, and a Date holding a dateTime lexical form).
fn declare_schema(db: &GraphDb) {
    init(db, Some(StarterKind::WorldModel), false).unwrap();
    // WorldModel already declares: Person.name/email, Organization.name/description,
    // Project.name/status/startDate(Date).
    add_property(
        db, "Project", "budget", "float64", false, false, None, None, None,
    )
    .unwrap();
    add_property(
        db,
        "Project",
        "headcount",
        "int64",
        false,
        false,
        None,
        None,
        None,
    )
    .unwrap();
    add_property(
        db, "Project", "active", "bool", false, false, None, None, None,
    )
    .unwrap();
    add_property(
        db,
        "Project",
        "launchedAt",
        "date",
        false,
        false,
        None,
        None,
        None,
    )
    .unwrap();
}

fn node(db: &GraphDb, label: &str, props: &[(&str, StoreValue)]) -> NodeId {
    let map: HashMap<String, StoreValue> = props
        .iter()
        .map(|(k, v)| ((*k).to_string(), v.clone()))
        .collect();
    let mut tx = db.begin_write().unwrap();
    let nid = tx.merge_node(label, map).unwrap();
    tx.commit().unwrap();
    nid
}

fn edge(db: &GraphDb, src: NodeId, dst: NodeId, rel: &str) {
    let mut tx = db.begin_write().unwrap();
    tx.create_edge(src, dst, rel, HashMap::new()).unwrap();
    tx.commit().unwrap();
}

/// Declare a class with no `iri` set, bypassing `define_class`'s validation
/// (which only rejects the `__SO_`-reserved prefix, not arbitrary characters)
/// so the default-minted class IRI (`{base}/schema/{name}`) is exercised with
/// a name that cannot form a valid IRI segment.
fn define_class_raw(db: &GraphDb, name: &str) {
    let props: HashMap<String, StoreValue> = [
        ("symbol_id".to_string(), sv(&format!("test-symbol-{name}"))),
        ("name".to_string(), sv(name)),
        ("description".to_string(), sv("")),
        ("status".to_string(), sv("active")),
        ("iri".to_string(), sv("")),
        ("created_at".to_string(), StoreValue::Int64(0)),
        ("updated_at".to_string(), StoreValue::Int64(0)),
    ]
    .into_iter()
    .collect();
    let mut tx = db.begin_write().unwrap();
    tx.merge_node(sparrowdb_ontology_core::namespace::CLASS_LABEL, props)
        .unwrap();
    tx.commit().unwrap();
}

/// Seed the fixture described above and checkpoint.
fn seed(db: &GraphDb) {
    declare_schema(db);

    // 5 Person
    let ada = node(
        db,
        "Person",
        &[
            ("name", sv("Ada Lovelace")),
            ("email", sv("ada@acme.example")),
        ],
    );
    let bob = node(
        db,
        "Person",
        &[("name", sv("Bob Stone")), ("email", sv("bob@acme.example"))],
    );
    let cleo = node(db, "Person", &[("name", sv("Cleo Vance"))]);
    let dan = node(db, "Person", &[("name", sv("Dan Reyes"))]);
    let eve = node(db, "Person", &[("name", sv("Eve Nakamura"))]);

    // 3 Organization
    let acme = node(
        db,
        "Organization",
        &[
            ("name", sv("Acme Corp")),
            ("description", sv("A manufacturer")),
        ],
    );
    let borealis = node(db, "Organization", &[("name", sv("Borealis Labs"))]);
    let cygnus = node(db, "Organization", &[("name", sv("Cygnus Group"))]);

    // 3 Project — Apollo carries one value of every datatype.
    let apollo = node(
        db,
        "Project",
        &[
            ("name", sv("Apollo")),
            ("status", sv("active")),
            ("startDate", sv("2026-03-01")),
            ("budget", sv("1250.5")),
            ("headcount", StoreValue::Int64(7)),
            ("active", StoreValue::Int64(1)),
            ("launchedAt", sv("2026-03-01T09:30:00Z")),
        ],
    );
    let beacon = node(
        db,
        "Project",
        &[("name", sv("Beacon")), ("status", sv("planning"))],
    );
    // Comet deliberately has no relationships — an entity with no edges must
    // still export.
    let _comet = node(db, "Project", &[("name", sv("Comet"))]);

    // 4 WORKS_FOR (Person -> Organization)
    edge(db, ada, acme, "WORKS_FOR");
    edge(db, bob, acme, "WORKS_FOR");
    edge(db, cleo, borealis, "WORKS_FOR");
    edge(db, dan, cygnus, "WORKS_FOR");

    // 4 KNOWS (Person -> Person)
    edge(db, ada, bob, "KNOWS");
    edge(db, bob, cleo, "KNOWS");
    edge(db, cleo, dan, "KNOWS");
    edge(db, dan, eve, "KNOWS");

    // 2 OWNS (Person -> Project)
    edge(db, ada, apollo, "OWNS");
    edge(db, cleo, beacon, "OWNS");

    // A whole class of bug in this engine only appears after a checkpoint.
    db.checkpoint().unwrap();
}

// ── Hand-derived expected totals ──────────────────────────────────────────────
//
// rdf:type triples — one per entity:                                       11
//
// Data-property triples, counted entity by entity:
//   Person:  Ada 2 (name,email) + Bob 2 (name,email) + Cleo 1 + Dan 1
//            + Eve 1                                                      =  7
//   Organization: Acme 2 (name,description) + Borealis 1 + Cygnus 1       =  4
//   Project: Apollo 7 (name,status,startDate,budget,headcount,active,
//                      launchedAt) + Beacon 2 (name,status) + Comet 1     = 10
//                                                                    subtotal 21
//
// Relationship triples:  4 WORKS_FOR + 4 KNOWS + 2 OWNS                   = 10
//
// TOTAL                                                       11 + 21 + 10 = 42
const EXPECTED_ENTITIES: usize = 11;
const EXPECTED_RELATIONSHIPS: usize = 10;
const EXPECTED_TRIPLES: usize = 42;

/// Parse Turtle into a set of N-Triples lines.
///
/// The exporter emits no blank nodes, so these graphs are *ground*: for ground
/// graphs, isomorphism coincides with set equality, which is both a stronger
/// and a far more debuggable assertion. `verify_rdf.py` additionally runs
/// rdflib's `isomorphic()` over the same files.
fn parse_to_set(ttl: &str) -> HashSet<String> {
    let mut out = HashSet::new();
    for r in oxttl::TurtleParser::new().for_slice(ttl.as_bytes()) {
        let t = r.expect("exported Turtle must parse cleanly");
        assert!(
            !matches!(t.subject, oxrdf::NamedOrBlankNode::BlankNode(_)),
            "the exporter must not emit blank-node subjects"
        );
        assert!(
            !matches!(t.object, oxrdf::Term::BlankNode(_)),
            "the exporter must not emit blank-node objects"
        );
        out.insert(t.to_string());
    }
    out
}

// ── Tests ─────────────────────────────────────────────────────────────────────

/// Acceptance: exported Turtle parses cleanly, and the graph has exactly the
/// hand-derived shape.
#[test]
fn export_emits_the_hand_derived_graph() {
    let dir = tempfile::tempdir().unwrap();
    let db = GraphDb::open(dir.path()).unwrap();
    seed(&db);

    let (ttl, report) = export_data_with_report(&db, BASE).unwrap();

    assert_eq!(report.entities_exported, EXPECTED_ENTITIES);
    assert_eq!(report.relationships_exported, EXPECTED_RELATIONSHIPS);
    assert_eq!(report.triples_emitted, EXPECTED_TRIPLES);
    assert_eq!(report.orphan_properties, 0);
    assert_eq!(report.dangling_edges, 0);
    assert!(report.orphan_labels.is_empty());

    let triples = parse_to_set(&ttl);
    assert_eq!(triples.len(), EXPECTED_TRIPLES);

    // Every subject is minted under {base}/{Class}/{node_id}. Node ids are not
    // asserted: they depend on label-creation order, which is not part of the
    // contract. The IRI *shape* is.
    let subjects: HashSet<String> = triples
        .iter()
        .map(|t| t.split(' ').next().unwrap().to_string())
        .collect();
    assert_eq!(subjects.len(), EXPECTED_ENTITIES);
    for s in &subjects {
        assert!(
            s.starts_with("<https://example.org/kb/Person/")
                || s.starts_with("<https://example.org/kb/Organization/")
                || s.starts_with("<https://example.org/kb/Project/"),
            "unexpected subject IRI shape: {s}"
        );
    }
    // 5 Person + 3 Organization + 3 Project.
    let count = |p: &str| subjects.iter().filter(|s| s.starts_with(p)).count();
    assert_eq!(count("<https://example.org/kb/Person/"), 5);
    assert_eq!(count("<https://example.org/kb/Organization/"), 3);
    assert_eq!(count("<https://example.org/kb/Project/"), 3);
}

/// Acceptance: the datatype mapping table, exercised end to end.
///
/// Each expected object term is written out from the fixture value and the
/// mapping table in the `rdf_data` module docs. These are object-side
/// assertions so they do not depend on node ids.
#[test]
fn datatypes_are_emitted_per_the_mapping_table() {
    let dir = tempfile::tempdir().unwrap();
    let db = GraphDb::open(dir.path()).unwrap();
    seed(&db);

    let ttl = export_data_turtle(&db, BASE).unwrap();
    let triples = parse_to_set(&ttl);

    let has = |pred: &str, obj: &str| {
        triples
            .iter()
            .any(|t| t.contains(&format!("<{BASE}/schema/{pred}> {obj}")))
    };
    let xsd = "http://www.w3.org/2001/XMLSchema#";

    // Project.budget is declared float64; Apollo's value is 1250.5.
    assert!(
        has("budget", &format!("\"1250.5\"^^<{xsd}double>")),
        "{ttl}"
    );
    // Project.headcount is declared int64; Apollo's value is 7.
    assert!(has("headcount", &format!("\"7\"^^<{xsd}integer>")), "{ttl}");
    // Project.active is declared bool; stored as Int64(1) -> "true".
    assert!(has("active", &format!("\"true\"^^<{xsd}boolean>")), "{ttl}");
    // Project.startDate is declared Date; "2026-03-01" has no time component.
    assert!(
        has("startDate", &format!("\"2026-03-01\"^^<{xsd}date>")),
        "{ttl}"
    );
    // Project.launchedAt is declared Date but holds a dateTime lexical form.
    // Tagging it ^^xsd:date would be an ill-typed literal, so the exporter
    // emits ^^xsd:dateTime. See rdf_data::date_datatype_for.
    assert!(
        has(
            "launchedAt",
            &format!("\"2026-03-01T09:30:00Z\"^^<{xsd}dateTime>")
        ),
        "{ttl}"
    );
    // Person.name is declared String. RDF 1.1 makes xsd:string implicit, so a
    // plain literal is what a conforming parser yields.
    assert!(has("name", "\"Ada Lovelace\""), "{ttl}");
}

/// **The headline acceptance criterion.**
///
/// export -> wipe -> import -> re-export, and the two exports are
/// graph-isomorphic.
///
/// "Wipe" is a fresh database on a fresh directory with the schema re-declared.
/// It is not an in-place Cypher delete: on SparrowDB 0.1.22
/// `MATCH (n:Label) DELETE n` removes the nodes but leaves their edges behind
/// (verified durable across `checkpoint()` and reopen), and `DETACH DELETE`
/// fails outright with "relationship type '__SO_HAS_PROPERTY' not found in
/// catalog". Building the round-trip on either would test the engine bug rather
/// than the exporter. Both are reported upstream; the exporter defends against
/// the dangling edges they leave (see `dangling_edges_are_skipped_and_reported`).
#[test]
fn roundtrip_is_graph_isomorphic() {
    let dir1 = tempfile::tempdir().unwrap();
    let db1 = GraphDb::open(dir1.path()).unwrap();
    seed(&db1);

    let export1 = export_data_turtle(&db1, BASE).unwrap();
    let jsonld1 = export_data_json_ld(&db1, BASE).unwrap();

    // ── wipe ──────────────────────────────────────────────────────────────────
    let dir2 = tempfile::tempdir().unwrap();
    let db2 = GraphDb::open(dir2.path()).unwrap();
    declare_schema(&db2);

    // Fresh database => fresh symbol_ids for every class and relation. The
    // exporter derives IRIs from class/relation *names*, never symbol_ids,
    // precisely so this survives.
    let report = import_data_turtle(&db2, &export1, ImportStrategy::Strict).unwrap();

    assert_eq!(report.entities_imported, EXPECTED_ENTITIES, "{report:?}");
    assert_eq!(
        report.relationships_imported, EXPECTED_RELATIONSHIPS,
        "{report:?}"
    );
    assert_eq!(report.entities_skipped, 0, "{report:?}");
    assert_eq!(report.relationships_skipped, 0, "{report:?}");
    assert_eq!(report.blank_nodes_skipped, 0, "{report:?}");
    // Strict must not have touched the ontology.
    assert_eq!(report.classes_declared, 0);
    assert_eq!(report.relations_declared, 0);
    assert_eq!(report.properties_declared, 0);

    db2.checkpoint().unwrap();

    let export2 = export_data_turtle(&db2, BASE).unwrap();

    // Ground graphs: isomorphism == set equality.
    let g1 = parse_to_set(&export1);
    let g2 = parse_to_set(&export2);
    assert_eq!(g1.len(), EXPECTED_TRIPLES);
    assert_eq!(
        g1,
        g2,
        "round-trip changed the graph.\nonly in first: {:?}\nonly in second: {:?}",
        g1.difference(&g2).collect::<Vec<_>>(),
        g2.difference(&g1).collect::<Vec<_>>()
    );

    // Subject IRIs must be *identical*, not merely isomorphic: node ids in db2
    // are freshly assigned, so this only holds because import persisted the
    // subject IRI onto each entity and export preferred it over minting.
    let subj = |g: &HashSet<String>| -> HashSet<String> {
        g.iter()
            .map(|t| t.split(' ').next().unwrap().to_string())
            .collect()
    };
    assert_eq!(subj(&g1), subj(&g2));

    // Cross-check with an independent RDF implementation when it is available.
    run_rdflib_check(&export1, &export2, &jsonld1);
}

/// Acceptance: JSON-LD is the same graph as the Turtle, and validates in a
/// JSON-LD 1.1 processor.
#[test]
fn json_ld_export_is_the_same_graph_as_turtle() {
    let dir = tempfile::tempdir().unwrap();
    let db = GraphDb::open(dir.path()).unwrap();
    seed(&db);

    let ttl = export_data_turtle(&db, BASE).unwrap();
    let jsonld = export_data_json_ld(&db, BASE).unwrap();

    // Structural checks that do not need a JSON-LD processor.
    let ctx = jsonld["@context"].as_object().expect("@context object");
    // Relations become terms with "@type": "@id" so their values are IRIs.
    for rel in ["WORKS_FOR", "KNOWS", "OWNS"] {
        assert_eq!(ctx[rel]["@type"], "@id", "{rel} should be an @id term");
        assert_eq!(ctx[rel]["@id"], format!("{BASE}/schema/{rel}"));
    }
    // Data properties become terms with an XSD datatype coercion.
    assert_eq!(
        ctx["headcount"]["@type"],
        "http://www.w3.org/2001/XMLSchema#integer"
    );
    assert_eq!(
        ctx["budget"]["@type"],
        "http://www.w3.org/2001/XMLSchema#double"
    );
    // Date terms carry no coercion: the datatype rides on each value, because
    // a Date property may hold either xsd:date or xsd:dateTime.
    assert!(ctx["startDate"].get("@type").is_none());

    let graph = jsonld["@graph"].as_array().expect("@graph array");
    assert_eq!(graph.len(), EXPECTED_ENTITIES);

    // The real check: rdflib parses the JSON-LD and compares it to the Turtle.
    run_rdflib_check(&ttl, &ttl, &jsonld);
}

/// Acceptance: the import report surfaces count imported, count skipped, and a
/// per-skip reason carrying the offending subject IRI.
#[test]
fn import_report_surfaces_every_skip_with_its_subject() {
    let dir = tempfile::tempdir().unwrap();
    let db = GraphDb::open(dir.path()).unwrap();
    declare_schema(&db);

    // Four subjects, three of which must be rejected for three distinct reasons.
    let ttl = format!(
        r#"
        <{BASE}/Person/1> a <{BASE}/schema/Person> ;
            <{BASE}/schema/name> "Valid Person" .

        <{BASE}/Person/2> a <{BASE}/schema/Wizard> ;
            <{BASE}/schema/name> "Unknown class" .

        <{BASE}/Person/3> a <{BASE}/schema/Person> ;
            <{BASE}/schema/name> "Bad property" ;
            <{BASE}/schema/favouriteColour> "teal" .

        <{BASE}/Person/4> <{BASE}/schema/name> "No rdf:type" .
        "#
    );

    let report = import_data_turtle(&db, &ttl, ImportStrategy::Strict).unwrap();

    // Only Person/1 is well formed.
    assert_eq!(report.entities_imported, 1, "{report:?}");
    assert_eq!(report.entities_skipped, 3, "{report:?}");
    assert_eq!(report.skips.len(), 3, "{report:?}");

    let reason_for = |iri: &str| -> String {
        report
            .skips
            .iter()
            .find(|s| s.subject == iri)
            .unwrap_or_else(|| panic!("no skip recorded for {iri}: {report:?}"))
            .reason
            .clone()
    };

    // Each skip names the offending subject IRI and says what to do about it.
    let r2 = reason_for(&format!("{BASE}/Person/2"));
    assert!(r2.contains("Unknown class"), "{r2}");
    assert!(r2.contains("AutoDeclare"), "{r2}");

    let r3 = reason_for(&format!("{BASE}/Person/3"));
    assert!(r3.contains("favouriteColour"), "{r3}");
    assert!(r3.contains("Valid:"), "{r3}");

    let r4 = reason_for(&format!("{BASE}/Person/4"));
    assert!(r4.contains("rdf:type"), "{r4}");
}

/// `ImportStrategy::AutoDeclare` declares what `Strict` rejects.
#[test]
fn auto_declare_creates_missing_schema() {
    let dir = tempfile::tempdir().unwrap();
    let db = GraphDb::open(dir.path()).unwrap();
    init(&db, Some(StarterKind::Blank), false).unwrap();

    // Two classes, one relation, two properties — none of them declared.
    let ttl = format!(
        r#"
        <{BASE}/Wizard/1> a <{BASE}/schema/Wizard> ;
            <{BASE}/schema/name> "Merlin" ;
            <{BASE}/schema/level> "42"^^<http://www.w3.org/2001/XMLSchema#integer> ;
            <{BASE}/schema/MENTORS> <{BASE}/Apprentice/1> .

        <{BASE}/Apprentice/1> a <{BASE}/schema/Apprentice> ;
            <{BASE}/schema/name> "Nimue" .
        "#
    );

    let report = import_data_turtle(&db, &ttl, ImportStrategy::AutoDeclare).unwrap();

    // 2 classes (Wizard, Apprentice); 1 relation (MENTORS); 3 properties
    // (Wizard.name, Wizard.level, Apprentice.name — name is declared once per
    // owning class, since properties are class-scoped in this ontology).
    assert_eq!(report.classes_declared, 2, "{report:?}");
    assert_eq!(report.relations_declared, 1, "{report:?}");
    assert_eq!(report.properties_declared, 3, "{report:?}");
    assert_eq!(report.entities_imported, 2, "{report:?}");
    assert_eq!(report.relationships_imported, 1, "{report:?}");
    assert_eq!(report.entities_skipped, 0, "{report:?}");

    db.checkpoint().unwrap();

    // The auto-declared schema must be good enough to export through.
    let (ttl2, r2) = export_data_with_report(&db, BASE).unwrap();
    assert_eq!(r2.entities_exported, 2);
    assert_eq!(r2.relationships_exported, 1);
    // 2 rdf:type + 3 data properties + 1 relationship = 6 triples.
    assert_eq!(r2.triples_emitted, 6, "{ttl2}");

    // `level` was auto-declared from ^^xsd:integer, so it must survive as an
    // integer. Asserted on the *parsed* form, not the Turtle text: Turtle
    // abbreviates `"42"^^xsd:integer` to a bare `42`, so string-matching the
    // serialization would test the serializer's shorthand rules, not the
    // exporter's datatype handling.
    let triples = parse_to_set(&ttl2);
    assert!(
        triples.iter().any(|t| t.contains(&format!(
            "<{BASE}/schema/level> \"42\"^^<http://www.w3.org/2001/XMLSchema#integer>"
        ))),
        "{triples:?}"
    );
}

/// A property written without an ontology declaration cannot be named in RDF
/// (the `col_<fnv1a32>` key is one-way). It must be counted and reported, not
/// dropped in silence.
#[test]
fn orphan_property_is_reported_not_dropped() {
    let dir = tempfile::tempdir().unwrap();
    let db = GraphDb::open(dir.path()).unwrap();
    declare_schema(&db);

    // "name" is declared on Person; "shoeSize" is not. Writing through the
    // low-level WriteTx API bypasses validation, which is exactly how an
    // undeclared column comes to exist.
    node(
        &db,
        "Person",
        &[("name", sv("Ada Lovelace")), ("shoeSize", sv("38"))],
    );
    db.checkpoint().unwrap();

    let (ttl, report) = export_data_with_report(&db, BASE).unwrap();

    assert_eq!(report.entities_exported, 1);
    // Exactly one unnameable column.
    assert_eq!(report.orphan_properties, 1, "{report:?}");
    // rdf:type + name. shoeSize cannot be emitted.
    assert_eq!(report.triples_emitted, 2, "{ttl}");
    assert!(!ttl.contains("shoeSize"));

    let issue = report
        .issues
        .iter()
        .find(|i| i.reason.contains("col_"))
        .unwrap_or_else(|| panic!("no orphan issue recorded: {report:?}"));
    assert!(issue.subject.starts_with(&format!("{BASE}/Person/")));
    assert!(issue.reason.contains("add_property"), "{}", issue.reason);
}

/// `dangling_edges` in [`ExportReport`] guards against a node being removed
/// while its edges survive it — which would otherwise let the exporter
/// assert facts about a deleted entity. On SparrowDB through 0.1.25,
/// `MATCH (n:Label) DELETE n` reached exactly that state: a packed `NodeId`
/// was passed where the edge-detection lookup expected a slot, so the lookup
/// silently found nothing and the delete proceeded, leaving `KNOWS`/`WORKS_FOR`
/// edges pointing at a node that no longer existed.
///
/// SparrowDB 0.1.27 (`#436`, upstream PR #512) fixed the lookup itself: plain
/// `DELETE` on a node with edges is now refused outright with
/// `NodeHasEdges` — not a value change, a bug fix to a check that was always
/// meant to fire — and `DETACH DELETE` removes the node and its edges
/// atomically. Traced through SparrowDB's WAL replay
/// (`sparrowdb-storage/src/wal/replay.rs`): mutations only replay for
/// transactions with a matching `Begin`+`Commit` pair, so a transaction that
/// fails the edge check (and therefore never reaches `Commit`) cannot leave a
/// partial trace, and a successful `DETACH DELETE`'s node- and edge-removal
/// mutations share one `txn_id` and replay together or not at all. So as of
/// 0.1.27 this scenario is not constructible through the public API by any
/// path — DELETE, DETACH DELETE, or crash/WAL-replay recovery.
///
/// The exporter's `dangling_edges` guard is kept regardless: it is cheap,
/// and it still protects a database created under a pre-0.1.27 SparrowDB
/// (whose on-disk state could already contain the inconsistency this test
/// used to construct) or any future storage-layer regression. This test now
/// asserts the *current* contract instead — DELETE is refused, DETACH DELETE
/// is clean — rather than a bug that no longer exists.
#[test]
fn delete_on_edge_bearing_node_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let db = GraphDb::open(dir.path()).unwrap();
    declare_schema(&db);

    let ada = node(&db, "Person", &[("name", sv("Ada Lovelace"))]);
    let bob = node(&db, "Person", &[("name", sv("Bob Stone"))]);
    edge(&db, ada, bob, "KNOWS");
    db.checkpoint().unwrap();

    let err = db.execute("MATCH (n:Person {name: 'Ada Lovelace'}) DELETE n");
    assert!(
        matches!(err, Err(sparrowdb::Error::NodeHasEdges { .. })),
        "expected NodeHasEdges, got: {err:?}"
    );

    // The refusal must be all-or-nothing: both nodes and the edge survive.
    let (ttl, report) = export_data_with_report(&db, BASE).unwrap();
    assert_eq!(report.entities_exported, 2, "{ttl}");
    assert_eq!(report.relationships_exported, 1, "{ttl}");
    assert_eq!(report.dangling_edges, 0, "{report:?}");
    assert!(ttl.contains("KNOWS"), "{ttl}");
}

/// `DETACH DELETE` removes a node and its edges atomically — the export
/// reflects a fully consistent graph afterward, with no orphaned/dangling
/// triples for the removed relationships.
#[test]
fn detach_delete_leaves_a_consistent_export() {
    let dir = tempfile::tempdir().unwrap();
    let db = GraphDb::open(dir.path()).unwrap();
    declare_schema(&db);

    let ada = node(&db, "Person", &[("name", sv("Ada Lovelace"))]);
    let bob = node(&db, "Person", &[("name", sv("Bob Stone"))]);
    let acme = node(&db, "Organization", &[("name", sv("Acme Corp"))]);
    edge(&db, ada, bob, "KNOWS");
    edge(&db, ada, acme, "WORKS_FOR");
    db.checkpoint().unwrap();

    // Sanity: 3 rdf:type + 3 name + 2 edges = 8 triples before the delete.
    assert_eq!(
        export_data_with_report(&db, BASE)
            .unwrap()
            .1
            .triples_emitted,
        8
    );

    db.execute("MATCH (n:Person {name: 'Ada Lovelace'}) DETACH DELETE n")
        .unwrap();
    db.checkpoint().unwrap();

    let (ttl, report) = export_data_with_report(&db, BASE).unwrap();

    // Bob and Acme remain: 2 rdf:type + 2 name = 4 triples. No dangling edges
    // — DETACH DELETE took KNOWS and WORKS_FOR with it.
    assert_eq!(report.entities_exported, 2, "{ttl}");
    assert_eq!(report.triples_emitted, 4, "{ttl}");
    assert_eq!(report.dangling_edges, 0, "{report:?}");
    assert_eq!(report.relationships_exported, 0);
    assert!(!ttl.contains("KNOWS"));
    assert!(!ttl.contains("WORKS_FOR"));
    assert!(ttl.contains("Bob Stone"), "{ttl}");
    assert!(ttl.contains("Acme Corp"), "{ttl}");
}

/// A node whose subject IRI cannot be parsed (here: a stored `__so_iri` with
/// an embedded space, which `NamedNode::new` rejects) must be skipped and
/// reported, not abort the entire export — the same contract the module
/// already honours for orphan labels/properties and dangling edges.
#[test]
fn unrepresentable_subject_iri_is_skipped_not_fatal() {
    let dir = tempfile::tempdir().unwrap();
    let db = GraphDb::open(dir.path()).unwrap();
    declare_schema(&db);

    let bad = node(
        &db,
        "Person",
        &[
            ("name", sv("Bad Iri")),
            ("__so_iri", sv("not a valid iri with spaces")),
        ],
    );
    let good = node(&db, "Person", &[("name", sv("Good Node"))]);
    edge(&db, bad, good, "KNOWS");
    db.checkpoint().unwrap();

    let (ttl, report) = export_data_with_report(&db, BASE).unwrap();

    // Only the good node exports: 1 rdf:type + 1 name = 2 triples.
    assert_eq!(report.entities_exported, 1, "{ttl}");
    assert_eq!(report.triples_emitted, 2, "{ttl}");
    assert_eq!(report.unrepresentable_iris, 1, "{report:?}");
    assert!(
        report
            .issues
            .iter()
            .any(|i| i.reason.contains("could not be represented in RDF")),
        "{report:?}"
    );
    // The KNOWS edge lost its source (the bad node was never inserted into
    // subject_by_id), so it's reported as dangling rather than half-exported.
    assert_eq!(report.dangling_edges, 1, "{report:?}");
    assert!(!ttl.contains("KNOWS"));
    assert!(!ttl.contains("Bad Iri"));
    assert!(ttl.contains("Good Node"), "{ttl}");
}

/// A class whose IRI (default-minted from its name) cannot be parsed — e.g. a
/// class name containing a space, which is not rejected by `define_class`
/// today — must skip that class's nodes and report it, not abort the whole
/// export. This is the same failure mode `unrepresentable_subject_iri_is_skipped_not_fatal`
/// covers, but at the class/label granularity instead of the per-node one.
#[test]
fn unrepresentable_class_iri_is_skipped_not_fatal() {
    let dir = tempfile::tempdir().unwrap();
    let db = GraphDb::open(dir.path()).unwrap();
    declare_schema(&db);

    define_class_raw(&db, "Online Account");
    add_property(
        &db,
        "Online Account",
        "handle",
        "string",
        false,
        false,
        None,
        None,
        None,
    )
    .unwrap();
    node(&db, "Online Account", &[("handle", sv("@example"))]);
    let person = node(&db, "Person", &[("name", sv("Ada Lovelace"))]);
    db.checkpoint().unwrap();

    let (ttl, report) = export_data_with_report(&db, BASE).unwrap();

    // Only Person exports: 1 rdf:type + 1 name = 2 triples.
    assert_eq!(report.entities_exported, 1, "{ttl}");
    assert_eq!(report.triples_emitted, 2, "{ttl}");
    assert_eq!(report.unrepresentable_iris, 1, "{report:?}");
    assert!(
        report.issues.iter().any(|i| i.subject == "Online Account"
            && i.reason.contains("could not be represented in RDF")),
        "{report:?}"
    );
    assert!(!ttl.contains("@example"));
    let _ = person;
}

/// Acceptance: `export_json_ld` (schema) behaviour is unchanged.
///
/// WS1 is additive. The schema exporter must still key its `@id` off
/// `so:{symbol_id}` and emit owl:Class / owl:ObjectProperty exactly as before.
#[test]
fn schema_export_json_ld_is_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let db = GraphDb::open(dir.path()).unwrap();
    seed(&db);

    let doc = sparrowdb_ontology_core::export_json_ld(&db).unwrap();
    let ctx = &doc["@context"];
    assert_eq!(ctx["owl"], "http://www.w3.org/2002/07/owl#");
    assert_eq!(ctx["so"], "http://sparrowontology.io/schema#");

    let graph = doc["@graph"].as_array().unwrap();
    // WorldModel declares 10 classes and 19 relations.
    let classes = graph.iter().filter(|n| n["@type"] == "owl:Class").count();
    let rels = graph
        .iter()
        .filter(|n| n["@type"] == "owl:ObjectProperty")
        .count();
    assert_eq!(classes, 10);
    assert_eq!(rels, 19);

    // Unchanged fallback: no explicit IRI => "so:" + symbol_id.
    let person = graph
        .iter()
        .find(|n| n["rdfs:label"] == "Person")
        .expect("Person class");
    assert!(
        person["@id"].as_str().unwrap().starts_with("so:"),
        "{person}"
    );
}

// ── rdflib cross-check ────────────────────────────────────────────────────────

/// Shell out to `tests/tools/verify_rdf.py` for an independent isomorphism
/// check using rdflib.
///
/// The native oxrdf assertions above always run and are the hard gate. This
/// adds a *second, independent* implementation's opinion, which is what the
/// spec asks for. If rdflib is not installed the script exits 77 and this is
/// reported as skipped — never silently treated as a pass.
fn run_rdflib_check(ttl1: &str, ttl2: &str, jsonld: &serde_json::Value) {
    let dir = tempfile::tempdir().unwrap();
    let p1 = dir.path().join("export1.ttl");
    let p2 = dir.path().join("export2.ttl");
    let pj = dir.path().join("export1.jsonld");
    std::fs::write(&p1, ttl1).unwrap();
    std::fs::write(&p2, ttl2).unwrap();
    std::fs::write(&pj, serde_json::to_string_pretty(jsonld).unwrap()).unwrap();

    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/tools/verify_rdf.py")
        .canonicalize()
        .expect("verify_rdf.py must exist");

    // SPARROW_RDFLIB_PYTHON lets CI point at an interpreter that has rdflib.
    let python = std::env::var("SPARROW_RDFLIB_PYTHON").unwrap_or_else(|_| "python3".to_string());
    let out = match std::process::Command::new(&python)
        .arg(&script)
        .arg(&p1)
        .arg(&p2)
        .arg(&pj)
        .output()
    {
        Ok(o) => o,
        Err(e) => {
            eprintln!("rdflib cross-check SKIPPED: cannot run {python}: {e}");
            return;
        }
    };

    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    match out.status.code() {
        Some(0) => eprintln!("rdflib cross-check PASSED: {}", stdout.trim()),
        // 77 is the script's "rdflib not installed" signal.
        Some(77) => eprintln!(
            "rdflib cross-check SKIPPED: rdflib not installed. \
             Set SPARROW_RDFLIB_PYTHON to an interpreter that has it."
        ),
        _ => panic!("rdflib cross-check FAILED:\n{stdout}\n{stderr}"),
    }
}
