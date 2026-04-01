//! Turtle / OWL import for sparrowdb-ontology-core.
//!
//! Parses a Turtle file and maps OWL / RDFS / schema.org constructs into the
//! SparrowDB ontology graph (`__SO_*` nodes and edges).
//!
//! ## Supported mappings
//!
//! | Turtle construct              | Maps to                              |
//! |-------------------------------|--------------------------------------|
//! | `owl:Class`, `rdfs:Class`, `schema:Class` via `rdf:type` | `__SO_Class` node |
//! | `owl:ObjectProperty` via `rdf:type` | `__SO_Relation` node |
//! | `owl:DatatypeProperty` via `rdf:type` | `add_property` on the domain class |
//! | `rdfs:subClassOf`             | `__SO_SUBCLASS_OF` edge              |
//! | `rdfs:label`                  | `name` property (prefer `@en`)       |
//! | `rdfs:comment`                | `description` property               |
//! | `rdfs:domain`                 | `__SO_DOMAIN` edge (subject to strategy) OR `add_property` owner |
//! | `rdfs:range` (class IRI)      | `__SO_RANGE` edge (subject to strategy) |
//! | `rdfs:range` (XSD datatype)   | property type for `owl:DatatypeProperty` |
//! | `schema:domainIncludes`       | `__SO_DOMAIN` edge (subject to strategy) |
//! | `schema:rangeIncludes`        | `__SO_RANGE` edge (subject to strategy) |
//! | `skos:altLabel`               | `__SO_Alias` node                    |

use std::collections::{HashMap, HashSet};

use oxrdf::{NamedOrBlankNode, Term};
use sparrowdb::GraphDb;
use sparrowdb_common::NodeId;
use sparrowdb_storage::node_store::Value as StoreValue;

use crate::error::SoError;
use crate::init::{add_alias, add_property, define_subclass};
use crate::model::{AliasKind, PropertyType, SymbolStatus};
use crate::namespace::{CLASS_LABEL, DOMAIN_REL, RANGE_REL, RELATION_LABEL};
use crate::resolution::resolve;
use crate::snapshot::export_schema;

// ── IRI constants ─────────────────────────────────────────────────────────────

const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
const OWL_CLASS: &str = "http://www.w3.org/2002/07/owl#Class";
const OWL_OBJ_PROP: &str = "http://www.w3.org/2002/07/owl#ObjectProperty";
const OWL_DATA_PROP: &str = "http://www.w3.org/2002/07/owl#DatatypeProperty";
const RDFS_CLASS: &str = "http://www.w3.org/2000/01/rdf-schema#Class";
const RDFS_SUBCLASS_OF: &str = "http://www.w3.org/2000/01/rdf-schema#subClassOf";
const RDFS_LABEL: &str = "http://www.w3.org/2000/01/rdf-schema#label";
const RDFS_COMMENT: &str = "http://www.w3.org/2000/01/rdf-schema#comment";
const RDFS_DOMAIN: &str = "http://www.w3.org/2000/01/rdf-schema#domain";
const RDFS_RANGE: &str = "http://www.w3.org/2000/01/rdf-schema#range";
const SKOS_ALT_LABEL: &str = "http://www.w3.org/2004/02/skos/core#altLabel";
const SCHEMA_CLASS: &str = "https://schema.org/Class";
const SCHEMA_DOM_INC: &str = "https://schema.org/domainIncludes";
const SCHEMA_RNG_INC: &str = "https://schema.org/rangeIncludes";

// ── Public types ──────────────────────────────────────────────────────────────

/// Options controlling how the Turtle import behaves.
#[derive(Debug, Clone)]
pub struct ImportOptions {
    /// Base IRI to resolve relative IRIs in the Turtle file.
    pub base_iri: Option<String>,
    /// How to handle multiple domain/range values on a single property.
    pub domain_range_strategy: DomainRangeStrategy,
}

/// Strategy for resolving `rdfs:domain` / `rdfs:range` (and schema.org variants)
/// when a property declares more than one domain or range class.
#[derive(Debug, Clone, PartialEq)]
pub enum DomainRangeStrategy {
    /// Take only the first domain/range encountered (good for strict OWL).
    FirstOnly,
    /// Record no domain/range if multiple exist (good for schema.org).
    Unconstrained,
}

impl Default for ImportOptions {
    fn default() -> Self {
        Self {
            base_iri: None,
            domain_range_strategy: DomainRangeStrategy::Unconstrained,
        }
    }
}

/// Summary of what was imported.
#[derive(Debug, Default)]
pub struct ImportSummary {
    pub classes_imported: usize,
    pub relations_imported: usize,
    pub subclasses_imported: usize,
    pub aliases_imported: usize,
    pub properties_imported: usize,
    pub warnings: Vec<String>,
    /// Names of `owl:DatatypeProperty` terms that were skipped because they
    /// had no resolvable `rdfs:domain`.
    pub skipped_no_domain_properties: Vec<String>,
}

// ── Main entry point ──────────────────────────────────────────────────────────

/// Parse `ttl` as Turtle and import all recognised OWL / RDFS / schema.org
/// constructs into `db`.
///
/// All failures that can be treated as recoverable (unknown subjects, cycles,
/// parse errors on individual triples) are recorded in `ImportSummary::warnings`
/// rather than causing the function to return `Err`.  Only storage-level errors
/// that affect the entire import bubble up as `Err(SoError)`.
pub fn import_turtle(
    db: &GraphDb,
    ttl: &str,
    opts: ImportOptions,
) -> Result<ImportSummary, SoError> {
    // ── Step 1: Parse all triples into in-memory maps ─────────────────────────

    let mut class_iris: HashSet<String> = HashSet::new();
    let mut obj_prop_iris: HashSet<String> = HashSet::new();
    let mut data_prop_iris: HashSet<String> = HashSet::new();

    // IRI → preferred label  (tracking language preference separately)
    let mut labels_en: HashMap<String, String> = HashMap::new();
    let mut labels_untagged: HashMap<String, String> = HashMap::new();
    let mut labels_first: HashMap<String, String> = HashMap::new();

    let mut comments: HashMap<String, String> = HashMap::new();
    let mut alt_labels: HashMap<String, Vec<String>> = HashMap::new();
    let mut subclass_pairs: Vec<(String, String)> = Vec::new();
    let mut domains: HashMap<String, Vec<String>> = HashMap::new();
    let mut ranges: HashMap<String, Vec<String>> = HashMap::new();

    let mut blank_node_count: usize = 0;
    let mut warnings: Vec<String> = Vec::new();

    // Build the parser
    let parser = if let Some(ref base) = opts.base_iri {
        match oxttl::TurtleParser::new().with_base_iri(base.clone()) {
            Ok(p) => p,
            Err(e) => {
                warnings.push(format!("Invalid base IRI '{base}': {e}"));
                oxttl::TurtleParser::new()
            }
        }
    } else {
        oxttl::TurtleParser::new()
    };

    for result in parser.for_slice(ttl.as_bytes()) {
        let triple = match result {
            Ok(t) => t,
            Err(e) => {
                warnings.push(format!("Parse error: {e}"));
                continue;
            }
        };

        // We only handle named-node subjects (skip blank nodes)
        let subject_iri = match &triple.subject {
            NamedOrBlankNode::NamedNode(n) => n.as_str().to_owned(),
            NamedOrBlankNode::BlankNode(_) => {
                blank_node_count += 1;
                continue;
            }
        };

        let predicate_iri = triple.predicate.as_str();

        match predicate_iri {
            // ── rdf:type declarations ─────────────────────────────────────
            p if p == RDF_TYPE => {
                if let Term::NamedNode(obj) = &triple.object {
                    let obj_str = obj.as_str();
                    match obj_str {
                        t if t == OWL_CLASS || t == RDFS_CLASS || t == SCHEMA_CLASS => {
                            class_iris.insert(subject_iri);
                        }
                        t if t == OWL_OBJ_PROP => {
                            obj_prop_iris.insert(subject_iri);
                        }
                        t if t == OWL_DATA_PROP => {
                            data_prop_iris.insert(subject_iri);
                        }
                        _ => {}
                    }
                }
            }

            // ── rdfs:subClassOf ───────────────────────────────────────────
            p if p == RDFS_SUBCLASS_OF => {
                if let Term::NamedNode(parent) = &triple.object {
                    subclass_pairs.push((subject_iri, parent.as_str().to_owned()));
                }
            }

            // ── rdfs:label ────────────────────────────────────────────────
            p if p == RDFS_LABEL => {
                if let Term::Literal(lit) = &triple.object {
                    let value = lit.value().to_owned();
                    match lit.language() {
                        Some(lang) if lang.starts_with("en") => {
                            labels_en.entry(subject_iri).or_insert(value);
                        }
                        None => {
                            labels_untagged.entry(subject_iri).or_insert(value);
                        }
                        Some(_) => {
                            labels_first.entry(subject_iri).or_insert(value);
                        }
                    }
                }
            }

            // ── rdfs:comment ──────────────────────────────────────────────
            p if p == RDFS_COMMENT => {
                if let Term::Literal(lit) = &triple.object {
                    let value = lit.value().to_owned();
                    match lit.language() {
                        Some(lang) if lang.starts_with("en") => {
                            // Prefer English; overwrite any non-English value
                            comments.insert(subject_iri, value);
                        }
                        None => {
                            comments.entry(subject_iri).or_insert(value);
                        }
                        Some(_) => {
                            comments.entry(subject_iri).or_insert(value);
                        }
                    }
                }
            }

            // ── rdfs:domain / schema:domainIncludes ───────────────────────
            p if p == RDFS_DOMAIN || p == SCHEMA_DOM_INC => {
                if let Term::NamedNode(cls) = &triple.object {
                    domains
                        .entry(subject_iri)
                        .or_default()
                        .push(cls.as_str().to_owned());
                }
            }

            // ── rdfs:range / schema:rangeIncludes ─────────────────────────
            p if p == RDFS_RANGE || p == SCHEMA_RNG_INC => {
                if let Term::NamedNode(cls) = &triple.object {
                    ranges
                        .entry(subject_iri)
                        .or_default()
                        .push(cls.as_str().to_owned());
                }
            }

            // ── skos:altLabel ─────────────────────────────────────────────
            p if p == SKOS_ALT_LABEL => {
                if let Term::Literal(lit) = &triple.object {
                    alt_labels
                        .entry(subject_iri)
                        .or_default()
                        .push(lit.value().to_owned());
                }
            }

            _ => {}
        }
    }

    if blank_node_count > 0 {
        warnings.push(format!(
            "Skipped {blank_node_count} triple(s) with blank-node subjects"
        ));
    }

    // ── Step 2: Build consolidated label map ──────────────────────────────────

    // Merge all IRIs seen (classes + props + anything with a label)
    let all_iris: HashSet<&String> = class_iris
        .iter()
        .chain(obj_prop_iris.iter())
        .chain(data_prop_iris.iter())
        .collect();
    let mut labels: HashMap<String, String> = HashMap::new();
    for iri in &all_iris {
        let label = labels_en
            .get(*iri)
            .or_else(|| labels_untagged.get(*iri))
            .or_else(|| labels_first.get(*iri))
            .cloned()
            .unwrap_or_else(|| local_name(iri));
        labels.insert((*iri).clone(), label);
    }

    // ── Step 3: Build IRI→name map ────────────────────────────────────────────

    let iri_to_name: HashMap<String, String> = all_iris
        .iter()
        .map(|iri| {
            let name = labels.get(*iri).cloned().unwrap_or_else(|| local_name(iri));
            ((*iri).clone(), name)
        })
        .collect();

    // ── Step 4: Write pass ────────────────────────────────────────────────────

    let mut classes_imported: usize = 0;
    let mut relations_imported: usize = 0;
    let mut subclasses_imported: usize = 0;
    let mut aliases_imported: usize = 0;
    let mut properties_imported: usize = 0;
    let mut skipped_no_domain_properties: Vec<String> = Vec::new();

    // Pre-build (owner_name, prop_name) → PropertyType map for type-drift checks on
    // DuplicateProperty.  Built once here to avoid re-querying the schema on every
    // duplicate hit.  Storage failures are fatal and propagated immediately.
    // Kept mutable so successful same-batch insertions are visible to subsequent
    // duplicate checks within the same import run.
    let mut existing_props: HashMap<(String, String), PropertyType> = if data_prop_iris.is_empty() {
        HashMap::new()
    } else {
        export_schema(db)?
            .properties
            .into_iter()
            .map(|p| ((p.owner_name, p.name), p.datatype))
            .collect()
    };

    // 4a. Import classes
    for iri in &class_iris {
        let name = iri_to_name
            .get(iri)
            .cloned()
            .unwrap_or_else(|| local_name(iri));
        let desc = comments.get(iri).cloned().unwrap_or_default();
        match write_class_node(db, &name, &desc, iri) {
            Ok(_) => classes_imported += 1,
            Err(e) => warnings.push(format!("class {name}: {e}")),
        }
    }

    // 4b. Import subclass pairs (after all classes are written)
    for (child_iri, parent_iri) in &subclass_pairs {
        let child_name = match iri_to_name.get(child_iri) {
            Some(n) => n.clone(),
            None => {
                warnings.push(format!("subclass: unknown child IRI {child_iri} — skipped"));
                continue;
            }
        };
        let parent_name = match iri_to_name.get(parent_iri) {
            Some(n) => n.clone(),
            None => {
                warnings.push(format!(
                    "subclass: unknown parent IRI {parent_iri} — skipped"
                ));
                continue;
            }
        };
        match define_subclass(db, &child_name, &parent_name) {
            Ok(_) => subclasses_imported += 1,
            Err(SoError::CycleDetected { .. }) => {
                warnings.push(format!("cycle skipped: {child_name} → {parent_name}"))
            }
            Err(SoError::Storage(sparrowdb_common::Error::AlreadyExists)) => {
                // Edge already present — idempotent
            }
            Err(e) => warnings.push(format!("subclass {child_name}→{parent_name}: {e}")),
        }
    }

    // 4c. Import relations (owl:ObjectProperty)
    for iri in &obj_prop_iris {
        let name = iri_to_name
            .get(iri)
            .cloned()
            .unwrap_or_else(|| local_name(iri));
        let desc = comments.get(iri).cloned().unwrap_or_default();
        let domain = resolve_domain_range(&domains, iri, &iri_to_name, &opts.domain_range_strategy);
        let range = resolve_domain_range(&ranges, iri, &iri_to_name, &opts.domain_range_strategy);
        match write_relation_node(db, &name, &desc, iri, domain.as_deref(), range.as_deref()) {
            Ok(_) => relations_imported += 1,
            Err(e) => warnings.push(format!("relation {name}: {e}")),
        }
    }

    // 4d. Import data properties (owl:DatatypeProperty → add_property)
    for iri in &data_prop_iris {
        let name = iri_to_name
            .get(iri)
            .cloned()
            .unwrap_or_else(|| local_name(iri));

        // Collect all resolvable domain classes (rdfs:domain or schema:domainIncludes).
        //
        // NOTE: DatatypeProperty intentionally does NOT apply resolve_domain_range / strategy
        // here.  Unlike ObjectProperty (which maps to a graph edge and can only have one
        // domain endpoint), a DatatypeProperty maps to add_property which attaches a scalar
        // field to a class node — it is safe and correct to add the same property to every
        // declared domain class.  The domain_range_strategy controls edge semantics only.
        // Tests `multi_domain_includes_drops_domain_unconstrained` and
        // `first_only_strategy_keeps_domain` both document this fanout behaviour.
        let domain_names: Vec<String> = domains
            .get(iri)
            .map(|v| {
                v.iter()
                    .filter_map(|d_iri| {
                        // Prefer name from the current import fragment; fall back to
                        // resolving the local name against the DB so that DatatypeProperty
                        // declarations that reference a class already in the DB (but not
                        // re-typed in this Turtle) are not incorrectly skipped.
                        iri_to_name.get(d_iri).cloned().or_else(|| {
                            let candidate = local_name(d_iri);
                            resolve(db, &candidate, AliasKind::Class)
                                .ok()
                                .map(|_| candidate)
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        if domain_names.is_empty() {
            warnings.push(format!("data property '{name}': no rdfs:domain — skipped"));
            skipped_no_domain_properties.push(name.clone());
            continue;
        }

        // Resolve XSD range → Sparrow property type.
        // Prefer the first XSD-namespace range IRI; fall back to the first range of
        // any kind so non-XSD custom ranges still produce "string" rather than being
        // silently skipped when an XSD range appears later in the list.
        let type_str = ranges
            .get(iri)
            .and_then(|v| {
                v.iter()
                    .find(|r| r.starts_with("http://www.w3.org/2001/XMLSchema#"))
                    .or_else(|| v.first())
            })
            .map(|xsd_iri| xsd_to_type_str(xsd_iri))
            .unwrap_or("string");

        let prop_description = comments
            .get(iri)
            .filter(|s| !s.is_empty())
            .map(String::as_str);
        let prop_iri = Some(iri.as_str());

        // Resolve incoming type once — used in both the success and duplicate paths.
        let incoming_type = crate::init::parse_property_type_str(type_str);

        // Import property on each domain class
        for owner in &domain_names {
            match add_property(
                db,
                owner,
                &name,
                type_str,
                false,
                false,
                None,
                prop_description,
                prop_iri,
            ) {
                Ok(_) => {
                    properties_imported += 1;
                    // Update cache so subsequent same-batch duplicates see this insertion.
                    existing_props.insert((owner.clone(), name.clone()), incoming_type.clone());
                }
                Err(SoError::DuplicateProperty { .. }) => {
                    // Check for type drift using the cache (pre-import DB state + same-batch inserts).
                    if let Some(existing_type) = existing_props.get(&(owner.clone(), name.clone()))
                    {
                        if existing_type != &incoming_type {
                            warnings.push(format!(
                                "data property '{name}' on '{owner}': type conflict \
                                 — existing={existing_type:?}, incoming={type_str}"
                            ));
                        }
                    }
                }
                Err(e) => warnings.push(format!("data property '{name}' on '{owner}': {e}")),
            }
        }
    }

    // 4e. Import aliases (skos:altLabel)
    for (iri, alts) in &alt_labels {
        let name = match iri_to_name.get(iri) {
            Some(n) => n.clone(),
            None => continue,
        };
        let kind = if class_iris.contains(iri) {
            AliasKind::Class
        } else if obj_prop_iris.contains(iri) {
            AliasKind::Relation
        } else {
            // data properties don't have an alias kind — skip
            continue;
        };
        for alt in alts {
            match add_alias(db, alt, kind.clone(), &name) {
                Ok(_) => aliases_imported += 1,
                Err(SoError::AliasConflict {
                    alias, existing, ..
                }) => {
                    warnings.push(format!(
                        "alias '{alias}' already registered for '{existing}' — skipped"
                    ));
                }
                Err(e) => warnings.push(format!("alias {name}←{alt}: {e}")),
            }
        }
    }

    Ok(ImportSummary {
        classes_imported,
        relations_imported,
        subclasses_imported,
        aliases_imported,
        properties_imported,
        warnings,
        skipped_no_domain_properties,
    })
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Map an XSD datatype IRI to a Sparrow property type string.
///
/// Only IRIs whose namespace is exactly `http://www.w3.org/2001/XMLSchema#` are
/// considered XSD; anything else falls through to `"string"` to avoid false
/// matches on custom IRIs that happen to share a suffix (e.g. `example.com/ns#int`).
fn xsd_to_type_str(xsd_iri: &str) -> &'static str {
    const XSD_NS: &str = "http://www.w3.org/2001/XMLSchema#";
    let local = match xsd_iri.strip_prefix(XSD_NS) {
        Some(l) => l,
        None => return "string",
    };
    match local {
        "integer" | "int" | "long" | "short" | "byte" | "nonNegativeInteger"
        | "positiveInteger" | "negativeInteger" | "nonPositiveInteger" | "unsignedLong"
        | "unsignedInt" | "unsignedShort" | "unsignedByte" => "int64",
        "decimal" | "float" | "double" => "float64",
        "boolean" => "bool",
        // xsd:date and xsd:dateTime carry a calendar date component → "date".
        // xsd:time (HH:MM:SS only, no date) has no Sparrow Date equivalent → "string".
        "date" | "dateTime" => "date",
        _ => "string",
    }
}

/// Extract the local name from an IRI (everything after the last `#` or `/`).
fn local_name(iri: &str) -> String {
    iri.rfind(['#', '/'])
        .map(|pos| iri[pos + 1..].to_owned())
        .unwrap_or_else(|| iri.to_owned())
}

/// Choose the domain or range class name for a property IRI given the strategy.
fn resolve_domain_range(
    map: &HashMap<String, Vec<String>>,
    prop_iri: &str,
    iri_to_name: &HashMap<String, String>,
    strategy: &DomainRangeStrategy,
) -> Option<String> {
    let values = map.get(prop_iri)?;
    match strategy {
        DomainRangeStrategy::FirstOnly => {
            values.first().and_then(|iri| iri_to_name.get(iri)).cloned()
        }
        DomainRangeStrategy::Unconstrained => {
            if values.len() == 1 {
                values.first().and_then(|iri| iri_to_name.get(iri)).cloned()
            } else {
                None
            }
        }
    }
}

// ── Low-level node writers ────────────────────────────────────────────────────
//
// These mirror the pattern used in snapshot.rs / init.rs: WriteTx merge_node
// (idempotent get-or-create) to bypass the Cypher-layer __SO_ reservation.

fn sv(s: &str) -> StoreValue {
    StoreValue::Bytes(s.as_bytes().to_vec())
}

fn iv(n: i64) -> StoreValue {
    StoreValue::Int64(n)
}

fn bv(b: bool) -> StoreValue {
    StoreValue::Int64(if b { 1 } else { 0 })
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before Unix epoch")
        .as_millis() as i64
}

fn kv(pairs: &[(&str, StoreValue)]) -> std::collections::HashMap<String, StoreValue> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect()
}

/// Write (or overwrite) a `__SO_Class` node. Returns the NodeId.
///
/// Uses `merge_node` which is idempotent: if a node with the same `symbol_id`
/// already exists it is updated; no duplicate is created.
fn write_class_node(
    db: &GraphDb,
    name: &str,
    description: &str,
    iri: &str,
) -> Result<NodeId, SoError> {
    let symbol_id = uuid::Uuid::new_v4().to_string();
    let now = now_ms();
    let mut tx = db.begin_write()?;
    let nid = tx.merge_node(
        CLASS_LABEL,
        kv(&[
            ("symbol_id", sv(&symbol_id)),
            ("name", sv(name)),
            ("description", sv(description)),
            ("status", sv("active")),
            ("iri", sv(iri)),
            ("created_at", iv(now)),
            ("updated_at", iv(now)),
        ]),
    )?;
    tx.commit()?;
    Ok(nid)
}

/// Write (or overwrite) a `__SO_Relation` node. Optionally creates
/// `__SO_DOMAIN` and `__SO_RANGE` edges if the target class nodes exist.
fn write_relation_node(
    db: &GraphDb,
    name: &str,
    description: &str,
    iri: &str,
    domain_name: Option<&str>,
    range_name: Option<&str>,
) -> Result<NodeId, SoError> {
    let symbol_id = uuid::Uuid::new_v4().to_string();
    let now = now_ms();
    let mut tx = db.begin_write()?;
    let rel_nid = tx.merge_node(
        RELATION_LABEL,
        kv(&[
            ("symbol_id", sv(&symbol_id)),
            ("name", sv(name)),
            ("description", sv(description)),
            ("status", sv("active")),
            ("directed", bv(true)),
            ("iri", sv(iri)),
            ("created_at", iv(now)),
            ("updated_at", iv(now)),
        ]),
    )?;
    tx.commit()?;

    // Create DOMAIN / RANGE edges if we can resolve the class node
    if let Some(dname) = domain_name {
        if let Ok(domain_nid) = get_class_node_id(db, dname) {
            let mut tx2 = db.begin_write()?;
            // Ignore duplicate-edge errors — idempotent intent
            let _ = tx2.create_edge(
                rel_nid,
                domain_nid,
                DOMAIN_REL,
                std::collections::HashMap::new(),
            );
            tx2.commit()?;
        }
    }
    if let Some(rname) = range_name {
        if let Ok(range_nid) = get_class_node_id(db, rname) {
            let mut tx3 = db.begin_write()?;
            let _ = tx3.create_edge(
                rel_nid,
                range_nid,
                RANGE_REL,
                std::collections::HashMap::new(),
            );
            tx3.commit()?;
        }
    }

    Ok(rel_nid)
}

/// Look up a `__SO_Class` node ID by name.
///
/// Uses the same dual-scan workaround as `init.rs` (SparrowDB ≤0.1.6
/// inline-filter regression).
fn get_class_node_id(db: &GraphDb, name: &str) -> Result<NodeId, SoError> {
    use sparrowdb_execution::Value as ExecValue;

    let q_names = format!("MATCH (n:{CLASS_LABEL}) RETURN n.name");
    let q_ids = format!("MATCH (n:{CLASS_LABEL}) RETURN id(n)");

    let names_r = match db.execute(&q_names) {
        Ok(r) => r,
        Err(sparrowdb_common::Error::InvalidArgument(ref msg)) if msg.contains("unknown label") => {
            return Err(SoError::UnknownSymbol {
                name: name.to_owned(),
                kind: "class".to_owned(),
                valid: vec![],
                closest_match: None,
                suggestion: None,
            });
        }
        Err(e) => return Err(SoError::Storage(e)),
    };
    let ids_r = db.execute(&q_ids).map_err(SoError::Storage)?;

    for (nr, ir) in names_r.rows.iter().zip(ids_r.rows.iter()) {
        if let (Some(ExecValue::String(n)), Some(ExecValue::Int64(id))) = (nr.first(), ir.first()) {
            if n == name {
                return Ok(NodeId(*id as u64));
            }
        }
    }
    Err(SoError::UnknownSymbol {
        name: name.to_owned(),
        kind: "class".to_owned(),
        valid: vec![],
        closest_match: None,
        suggestion: None,
    })
}

// ── Status helper (used only for completeness; status is always "active") ─────

fn _status_str(_: &SymbolStatus) -> &'static str {
    "active"
}
