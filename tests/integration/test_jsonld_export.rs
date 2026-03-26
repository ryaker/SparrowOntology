use std::collections::HashMap;

use sparrowdb::GraphDb;
use sparrowdb_ontology_core::{
    add_alias, add_property, define_subclass, export_json_ld, init,
    model::{AliasKind, OntologyClass, OntologyRelation},
    StarterKind,
};
use sparrowdb_storage::node_store::Value as StoreValue;

// ── Helpers ───────────────────────────────────────────────────────────────────

fn sv(s: &str) -> StoreValue {
    StoreValue::Bytes(s.as_bytes().to_vec())
}

fn iv(n: i64) -> StoreValue {
    StoreValue::Int64(n)
}

/// Open a blank-initialised DB (no starter classes or relations).
fn blank_db() -> (tempfile::TempDir, GraphDb) {
    let dir = tempfile::tempdir().unwrap();
    let db = GraphDb::open(dir.path()).unwrap();
    init(&db, Some(StarterKind::Blank), false).unwrap();
    (dir, db)
}

/// Seed a class node via WriteTx (mirrors init.rs seed_class pattern).
/// `iri` may be `None` (stored as empty string) or `Some(iri_str)`.
fn seed_class(db: &GraphDb, c: &OntologyClass) {
    let iri = c.iri.as_deref().unwrap_or("");
    let desc = c.description.as_deref().unwrap_or("");
    let mut props = HashMap::new();
    props.insert("symbol_id".to_string(), sv(&c.symbol_id));
    props.insert("name".to_string(), sv(&c.name));
    props.insert("description".to_string(), sv(desc));
    props.insert("status".to_string(), sv("active"));
    props.insert("iri".to_string(), sv(iri));
    props.insert("created_at".to_string(), iv(c.created_at));
    props.insert("updated_at".to_string(), iv(c.updated_at));
    let mut tx = db.begin_write().unwrap();
    tx.merge_node("__SO_Class", props).unwrap();
    tx.commit().unwrap();
}

/// Seed a relation node + DOMAIN and RANGE edges via WriteTx.
/// Requires domain and range class nodes to already exist.
fn seed_relation(db: &GraphDb, r: &OntologyRelation) {
    let iri = r.iri.as_deref().unwrap_or("");
    let desc = r.description.as_deref().unwrap_or("");
    let mut props = HashMap::new();
    props.insert("symbol_id".to_string(), sv(&r.symbol_id));
    props.insert("name".to_string(), sv(&r.name));
    props.insert("description".to_string(), sv(desc));
    props.insert("status".to_string(), sv("active"));
    props.insert("directed".to_string(), iv(if r.directed { 1 } else { 0 }));
    props.insert("iri".to_string(), sv(iri));
    props.insert("created_at".to_string(), iv(r.created_at));
    props.insert("updated_at".to_string(), iv(r.updated_at));
    let mut tx = db.begin_write().unwrap();
    let rel_node_id = tx.merge_node("__SO_Relation", props).unwrap();
    tx.commit().unwrap();

    // Create DOMAIN edge (relation → domain class)
    if !r.domain.is_empty() {
        let domain_id = get_class_node_id(db, &r.domain);
        if let Some(domain_id) = domain_id {
            let mut tx = db.begin_write().unwrap();
            tx.create_edge(rel_node_id, domain_id, "__SO_DOMAIN", HashMap::new())
                .unwrap();
            tx.commit().unwrap();
        }
    }

    // Create RANGE edge (relation → range class)
    if !r.range.is_empty() {
        let range_id = get_class_node_id(db, &r.range);
        if let Some(range_id) = range_id {
            let mut tx = db.begin_write().unwrap();
            tx.create_edge(rel_node_id, range_id, "__SO_RANGE", HashMap::new())
                .unwrap();
            tx.commit().unwrap();
        }
    }
}

/// Return the NodeId for a class by scanning names (two-scan workaround).
fn get_class_node_id(db: &GraphDb, name: &str) -> Option<sparrowdb_common::NodeId> {
    use sparrowdb_execution::Value as ExecVal;
    let names_r = db.execute("MATCH (n:__SO_Class) RETURN n.name").ok()?;
    let ids_r = db.execute("MATCH (n:__SO_Class) RETURN id(n)").ok()?;
    for (nr, ir) in names_r.rows.iter().zip(ids_r.rows.iter()) {
        if let (Some(ExecVal::String(n)), Some(ExecVal::Int64(id))) = (nr.first(), ir.first()) {
            if n == name {
                return Some(sparrowdb_common::NodeId(*id as u64));
            }
        }
    }
    None
}

// ── Helper to pull @graph out of a JSON-LD value ─────────────────────────────

fn get_graph(doc: &serde_json::Value) -> &Vec<serde_json::Value> {
    doc["@graph"].as_array().expect("@graph must be an array")
}

fn get_context(doc: &serde_json::Value) -> &serde_json::Map<String, serde_json::Value> {
    doc["@context"]
        .as_object()
        .expect("@context must be an object")
}

/// Find a node in @graph by rdfs:label value.
fn find_node_by_label<'a>(
    graph: &'a Vec<serde_json::Value>,
    label: &str,
) -> Option<&'a serde_json::Value> {
    graph
        .iter()
        .find(|node| node["rdfs:label"].as_str() == Some(label))
}

// ══════════════════════════════════════════════════════════════════════════════
// Tests
// ══════════════════════════════════════════════════════════════════════════════

// ── 1. Empty ontology ─────────────────────────────────────────────────────────

#[test]
fn empty_ontology_has_all_five_context_prefixes() {
    let (_dir, db) = blank_db();
    let doc = export_json_ld(&db).expect("export_json_ld failed on blank DB");
    let ctx = get_context(&doc);

    assert_eq!(
        ctx.get("owl").and_then(|v| v.as_str()),
        Some("http://www.w3.org/2002/07/owl#")
    );
    assert_eq!(
        ctx.get("rdfs").and_then(|v| v.as_str()),
        Some("http://www.w3.org/2000/01/rdf-schema#")
    );
    assert_eq!(
        ctx.get("xsd").and_then(|v| v.as_str()),
        Some("http://www.w3.org/2001/XMLSchema#")
    );
    assert_eq!(
        ctx.get("skos").and_then(|v| v.as_str()),
        Some("http://www.w3.org/2004/02/skos/core#")
    );
    assert_eq!(
        ctx.get("so").and_then(|v| v.as_str()),
        Some("http://sparrowontology.io/schema#")
    );
}

#[test]
fn empty_ontology_has_empty_graph_array() {
    let (_dir, db) = blank_db();
    let doc = export_json_ld(&db).expect("export_json_ld failed on blank DB");
    let graph = get_graph(&doc);
    assert!(
        graph.is_empty(),
        "expected empty @graph on blank DB, got {} entries",
        graph.len()
    );
}

// ── 2. Single class, no IRI ───────────────────────────────────────────────────

#[test]
fn single_class_no_iri_has_correct_type_and_label() {
    let (_dir, db) = blank_db();
    let c = OntologyClass::new("Person", "A human individual");
    seed_class(&db, &c);

    let doc = export_json_ld(&db).unwrap();
    let graph = get_graph(&doc);
    assert_eq!(graph.len(), 1, "expected exactly 1 graph node");

    let node = &graph[0];
    assert_eq!(
        node["@type"].as_str(),
        Some("owl:Class"),
        "@type must be owl:Class"
    );
    assert_eq!(
        node["rdfs:label"].as_str(),
        Some("Person"),
        "rdfs:label must be 'Person'"
    );

    let id = node["@id"].as_str().expect("@id must be a string");
    assert!(
        id.starts_with("so:"),
        "@id should start with 'so:' when no IRI is set, got: {id}"
    );
}

// ── 3. Single class with IRI ──────────────────────────────────────────────────

#[test]
fn single_class_with_iri_uses_iri_as_id() {
    let (_dir, db) = blank_db();
    let mut c = OntologyClass::new("Organization", "A legal entity");
    c.iri = Some("https://schema.org/Organization".to_string());
    seed_class(&db, &c);

    let doc = export_json_ld(&db).unwrap();
    let graph = get_graph(&doc);
    assert_eq!(graph.len(), 1);

    let node = &graph[0];
    assert_eq!(
        node["@id"].as_str(),
        Some("https://schema.org/Organization"),
        "@id must be the IRI when one is set"
    );
}

// ── 4. Class with description ─────────────────────────────────────────────────

#[test]
fn class_with_description_includes_rdfs_comment() {
    let (_dir, db) = blank_db();
    let c = OntologyClass::new("Document", "A written or digital record");
    seed_class(&db, &c);

    let doc = export_json_ld(&db).unwrap();
    let graph = get_graph(&doc);
    let node = find_node_by_label(graph, "Document").expect("Document node not found");
    assert_eq!(
        node["rdfs:comment"].as_str(),
        Some("A written or digital record"),
        "rdfs:comment must be present and match the description"
    );
}

// ── 5. Class with aliases ─────────────────────────────────────────────────────

#[test]
fn class_with_aliases_includes_skos_alt_label_array() {
    let (_dir, db) = blank_db();
    let c = OntologyClass::new("Person", "A human individual");
    seed_class(&db, &c);

    add_alias(&db, "Human", AliasKind::Class, "Person").unwrap();
    add_alias(&db, "Individual", AliasKind::Class, "Person").unwrap();

    let doc = export_json_ld(&db).unwrap();
    let graph = get_graph(&doc);
    let node = find_node_by_label(graph, "Person").expect("Person node not found");

    let alt_labels = node["skos:altLabel"]
        .as_array()
        .expect("skos:altLabel must be an array");
    assert_eq!(
        alt_labels.len(),
        2,
        "expected 2 aliases, got {}",
        alt_labels.len()
    );

    let labels: Vec<&str> = alt_labels.iter().filter_map(|v| v.as_str()).collect();
    assert!(
        labels.contains(&"Human"),
        "Human alias missing from skos:altLabel"
    );
    assert!(
        labels.contains(&"Individual"),
        "Individual alias missing from skos:altLabel"
    );
}

// ── 6. Subclass relationship ──────────────────────────────────────────────────

#[test]
fn subclass_entry_includes_rdfs_subclass_of_pointing_to_parent() {
    let (_dir, db) = blank_db();

    let animal = OntologyClass::new("Animal", "A living creature");
    let dog = OntologyClass::new("Dog", "A domestic canine");
    seed_class(&db, &animal);
    seed_class(&db, &dog);

    define_subclass(&db, "Dog", "Animal").unwrap();

    let doc = export_json_ld(&db).unwrap();
    let graph = get_graph(&doc);

    // Get Animal's @id to verify the subClassOf reference
    let animal_node = find_node_by_label(graph, "Animal").expect("Animal node not found");
    let animal_id = animal_node["@id"]
        .as_str()
        .expect("Animal @id must be a string");

    let dog_node = find_node_by_label(graph, "Dog").expect("Dog node not found");
    let subclass_of = dog_node["rdfs:subClassOf"]
        .as_object()
        .expect("rdfs:subClassOf must be an object with @id");
    let parent_ref = subclass_of
        .get("@id")
        .and_then(|v| v.as_str())
        .expect("rdfs:subClassOf must have an @id key");

    assert_eq!(
        parent_ref, animal_id,
        "Dog's rdfs:subClassOf @id should point to Animal's @id"
    );
}

// ── 7. Relation with domain and range ─────────────────────────────────────────

#[test]
fn relation_includes_rdfs_domain_and_rdfs_range() {
    let (_dir, db) = blank_db();

    let a = OntologyClass::new("ClassA", "Domain class");
    let b = OntologyClass::new("ClassB", "Range class");
    seed_class(&db, &a);
    seed_class(&db, &b);

    let rel = OntologyRelation::new("KNOWS", "ClassA", "ClassB");
    seed_relation(&db, &rel);

    let doc = export_json_ld(&db).unwrap();
    let graph = get_graph(&doc);

    let rel_node = find_node_by_label(graph, "KNOWS").expect("KNOWS relation not found in @graph");
    assert_eq!(
        rel_node["@type"].as_str(),
        Some("owl:ObjectProperty"),
        "@type must be owl:ObjectProperty"
    );

    // Verify rdfs:domain
    let domain = rel_node["rdfs:domain"]
        .as_object()
        .expect("rdfs:domain must be an object with @id");
    let domain_id = domain
        .get("@id")
        .and_then(|v| v.as_str())
        .expect("rdfs:domain must have @id");

    let a_node = find_node_by_label(graph, "ClassA").expect("ClassA node not found");
    let a_id = a_node["@id"].as_str().unwrap();
    assert_eq!(
        domain_id, a_id,
        "rdfs:domain @id must point to ClassA's @id"
    );

    // Verify rdfs:range
    let range = rel_node["rdfs:range"]
        .as_object()
        .expect("rdfs:range must be an object with @id");
    let range_id = range
        .get("@id")
        .and_then(|v| v.as_str())
        .expect("rdfs:range must have @id");

    let b_node = find_node_by_label(graph, "ClassB").expect("ClassB node not found");
    let b_id = b_node["@id"].as_str().unwrap();
    assert_eq!(range_id, b_id, "rdfs:range @id must point to ClassB's @id");
}

// ── 8. Relation with IRI ──────────────────────────────────────────────────────

#[test]
fn relation_with_iri_uses_iri_as_id() {
    let (_dir, db) = blank_db();

    let person = OntologyClass::new("Person", "A human individual");
    let org = OntologyClass::new("Organization", "A legal entity");
    seed_class(&db, &person);
    seed_class(&db, &org);

    let mut rel = OntologyRelation::new("KNOWS", "Person", "Organization");
    rel.iri = Some("https://schema.org/knows".to_string());
    seed_relation(&db, &rel);

    let doc = export_json_ld(&db).unwrap();
    let graph = get_graph(&doc);

    let rel_node = find_node_by_label(graph, "KNOWS").expect("KNOWS relation not found in @graph");
    assert_eq!(
        rel_node["@id"].as_str(),
        Some("https://schema.org/knows"),
        "relation @id must be the IRI when one is set"
    );
}

// ── 9. Required/allowed properties omitted when empty ─────────────────────────

#[test]
fn class_without_properties_omits_so_required_and_allowed_properties_keys() {
    let (_dir, db) = blank_db();
    let c = OntologyClass::new("EmptyClass", "A class with no properties");
    seed_class(&db, &c);

    let doc = export_json_ld(&db).unwrap();
    let graph = get_graph(&doc);
    let node = find_node_by_label(graph, "EmptyClass").expect("EmptyClass not found");

    assert!(
        node.get("so:requiredProperties").is_none(),
        "so:requiredProperties must be absent when class has no properties"
    );
    assert!(
        node.get("so:allowedProperties").is_none(),
        "so:allowedProperties must be absent when class has no properties"
    );
}

// ── 10. Multiple classes and relations graph length ───────────────────────────

#[test]
fn graph_length_equals_total_classes_plus_relations() {
    let (_dir, db) = blank_db();

    // 3 classes
    seed_class(&db, &OntologyClass::new("Alpha", "Class alpha"));
    seed_class(&db, &OntologyClass::new("Beta", "Class beta"));
    seed_class(&db, &OntologyClass::new("Gamma", "Class gamma"));

    // 2 relations
    let r1 = OntologyRelation::new("REL_ONE", "Alpha", "Beta");
    let r2 = OntologyRelation::new("REL_TWO", "Beta", "Gamma");
    seed_relation(&db, &r1);
    seed_relation(&db, &r2);

    let doc = export_json_ld(&db).unwrap();
    let graph = get_graph(&doc);

    assert_eq!(
        graph.len(),
        5,
        "expected @graph to have 5 entries (3 classes + 2 relations), got {}",
        graph.len()
    );
}

// ── Bonus: class with required property includes so:requiredProperties ─────────

#[test]
fn class_with_required_property_includes_so_required_properties() {
    let (_dir, db) = blank_db();
    let c = OntologyClass::new("Contract", "A legal agreement");
    seed_class(&db, &c);

    add_property(&db, "Contract", "title", "string", true, false, None).unwrap();

    let doc = export_json_ld(&db).unwrap();
    let graph = get_graph(&doc);
    let node = find_node_by_label(graph, "Contract").expect("Contract not found");

    let req_props = node["so:requiredProperties"]
        .as_array()
        .expect("so:requiredProperties must be an array when required properties exist");
    let names: Vec<&str> = req_props.iter().filter_map(|v| v.as_str()).collect();
    assert!(
        names.contains(&"title"),
        "title must appear in so:requiredProperties"
    );
}

// ── Bonus: class without description omits rdfs:comment ───────────────────────

#[test]
fn class_without_description_omits_rdfs_comment() {
    let (_dir, db) = blank_db();
    // OntologyClass::new takes a name and description, but an empty description
    // should result in no rdfs:comment in the output.
    let mut c = OntologyClass::new("NoDesc", "");
    c.description = None;
    seed_class(&db, &c);

    let doc = export_json_ld(&db).unwrap();
    let graph = get_graph(&doc);
    let node = find_node_by_label(graph, "NoDesc").expect("NoDesc not found");

    assert!(
        node.get("rdfs:comment").is_none(),
        "rdfs:comment must be absent when description is empty/None"
    );
}
