//! Instance-data RDF export and import (WS1).
//!
//! Where [`crate::jsonld::export_json_ld`] projects the *schema* (owl:Class,
//! owl:ObjectProperty, …), this module projects the *data*: the actual entity
//! nodes and relationship edges stored in a SparrowDB database.
//!
//! This mirrors Vault-LD's two-file convention: `schema.ttl` comes from
//! `export_json_ld` / the schema layer, `data.ttl` comes from
//! [`export_data_turtle`].
//!
//! # Public API
//!
//! - [`export_data_turtle`] — entities and relationships as Turtle.
//! - [`export_data_json_ld`] — the same graph as JSON-LD 1.1.
//! - [`import_data_turtle`] — the inverse, through the validated write path.
//! - [`export_data_with_report`] — Turtle plus an [`ExportReport`] describing
//!   everything that could *not* be represented (orphans, dangling edges).
//!
//! # IRI minting and round-trip stability
//!
//! An entity's subject IRI is, in priority order:
//!
//! 1. the IRI stored on the entity in the reserved
//!    [`SOURCE_IRI_KEY`](crate::namespace::SOURCE_IRI_KEY) (`__so_iri`) property, if present;
//! 2. otherwise minted as `{base_iri}/{ClassName}/{node_id}`.
//!
//! [`import_data_turtle`] *persists* the subject IRI it read onto each entity
//! it creates. That is what makes `export → wipe → import → re-export`
//! reproduce the same subject IRIs: minting only ever happens on the first
//! export of a never-imported entity. Without this, re-import would assign
//! fresh node ids, mint different IRIs, and the second export would not be
//! isomorphic to the first.
//!
//! Export itself is strictly **read-only** — it never writes minted IRIs back
//! to the database.
//!
//! `node_id` is the full label-scoped [`sparrowdb_common::NodeId`]
//! (`(label_id << 32) | slot`), never the bare slot. Organisation slot 0 and
//! Person slot 0 are different nodes with ids `0` and `4294967296`; using the
//! bare slot would collapse them onto one subject.
//!
//! # Datatype mapping
//!
//! Ontology [`PropertyType`] → XSD datatype used on export:
//!
//! | `PropertyType` | XSD datatype on export        | Notes |
//! |----------------|-------------------------------|-------|
//! | `String`       | `xsd:string`                  | |
//! | `Int64`        | `xsd:integer`                 | import also accepts int/long/short/byte/unsigned*/nonNegativeInteger/… |
//! | `Float64`      | `xsd:double`                  | import also accepts `xsd:decimal`, `xsd:float` |
//! | `Bool`         | `xsd:boolean`                 | stored as `Int64` 0/1, emitted as `false`/`true` |
//! | `Date`         | `xsd:date` or `xsd:dateTime`  | chosen by lexical form — see below |
//! | `Variant`      | `xsd:string`                  | lossy: re-import yields `String`, not `Variant` |
//!
//! This table is the inverse of
//! [`crate::turtle_import::xsd_to_type_str`]. The two are in different modules,
//! so they are pinned together by
//! `tests::datatype_mapping_is_a_fixed_point`, which round-trips every
//! `PropertyType` through export → `xsd_to_type_str` → `property_type_from_str`
//! and asserts the result is unchanged. If either side is edited without the
//! other, that test fails.
//!
//! ## `xsd:date` vs `xsd:dateTime`
//!
//! `PropertyType::Date` covers both, because `xsd_to_type_str` maps *both*
//! `xsd:date` and `xsd:dateTime` onto `"date"`, and dates are stored as plain
//! strings (`validation.rs`: "dates stored as strings in v1" — `PropertyValue`
//! has no `Date` variant).
//!
//! Emitting a stored `"2026-03-01T09:30:00Z"` as `^^xsd:date` would produce an
//! **ill-typed literal** — a malformed lexical form for that datatype. So the
//! exporter inspects the stored lexical form and emits `xsd:dateTime` when the
//! value carries a time component and `xsd:date` otherwise. This keeps every
//! emitted literal well-formed and makes date round-trips exact. See
//! [`date_datatype_for`].
//!
//! # Only ontology-declared properties are exportable
//!
//! SparrowDB stores property names as `col_<fnv1a32(name)>` column keys, and
//! that hash is **one-way**. To read a property back *by name* this module
//! builds a reverse table: it takes every property the ontology declares (via
//! [`export_schema`]), hashes each name, and matches the resulting `col_` key.
//!
//! The consequence is a real limit: a property written directly through the
//! low-level `WriteTx` API without a matching ontology declaration cannot be
//! recovered by name and therefore cannot be exported. Such columns are
//! **counted and reported** as [`ExportReport::orphan_properties`] rather than
//! dropped in silence.
//!
//! # Non-goals (WS1)
//!
//! No SPARQL engine, no named graphs, no reasoning, and no blank-node
//! preservation on import — blank nodes are flagged in the report and skipped,
//! per the spirit of Vault-LD §5.6.

use std::collections::{BTreeMap, HashMap, HashSet};

use oxrdf::{Literal, NamedNode, Term, Triple};
use serde_json::{json, Map as JsMap, Value as JsVal};
use sparrowdb::GraphDb;
use sparrowdb_common::NodeId;
use sparrowdb_execution::Value as ExecValue;
use sparrowdb_storage::node_store::Value as StoreValue;

use crate::error::SoError;
use crate::model::{AliasKind, OntologyProperty, PropertyType, PropertyValue};
use crate::namespace::{SOURCE_IRI_KEY, SOURCE_LABEL_KEY, SO_NAMESPACE};
use crate::resolution::resolve;
use crate::snapshot::{export_schema, SchemaSnapshot};
use crate::turtle_import::{write_class_node, write_relation_node, xsd_to_type_str};
use crate::validation::ValidationContext;

// ── Vocabulary constants ──────────────────────────────────────────────────────

const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
const XSD_NS: &str = "http://www.w3.org/2001/XMLSchema#";

const XSD_STRING: &str = "http://www.w3.org/2001/XMLSchema#string";
const XSD_INTEGER: &str = "http://www.w3.org/2001/XMLSchema#integer";
const XSD_DOUBLE: &str = "http://www.w3.org/2001/XMLSchema#double";
const XSD_BOOLEAN: &str = "http://www.w3.org/2001/XMLSchema#boolean";
const XSD_DATE: &str = "http://www.w3.org/2001/XMLSchema#date";
const XSD_DATE_TIME: &str = "http://www.w3.org/2001/XMLSchema#dateTime";

// ── Datatype mapping ──────────────────────────────────────────────────────────

/// Map an ontology [`PropertyType`] to the XSD datatype IRI used on export.
///
/// This is the inverse of [`crate::turtle_import::xsd_to_type_str`]; see the
/// module docs for the full table and for the anti-drift test that pins the two
/// together.
pub fn property_type_to_xsd(t: &PropertyType) -> &'static str {
    match t {
        PropertyType::String => XSD_STRING,
        PropertyType::Int64 => XSD_INTEGER,
        PropertyType::Float64 => XSD_DOUBLE,
        PropertyType::Bool => XSD_BOOLEAN,
        PropertyType::Date => XSD_DATE,
        // Variant carries no static type information; export as a plain string.
        // Lossy: a re-import sees xsd:string and yields PropertyType::String.
        PropertyType::Variant => XSD_STRING,
    }
}

/// Choose `xsd:date` or `xsd:dateTime` for a `PropertyType::Date` value by
/// looking at its stored lexical form.
///
/// Dates are stored as strings, so a `Date` property may legitimately hold
/// either. Emitting a `dateTime` lexical form tagged `^^xsd:date` would be an
/// ill-typed literal, so the datatype follows the value.
fn date_datatype_for(lexical: &str) -> &'static str {
    // xsd:dateTime requires a 'T' separator between the date and time parts.
    if lexical.contains('T') {
        XSD_DATE_TIME
    } else {
        XSD_DATE
    }
}

/// SparrowDB stores user-facing property names as `col_<fnv1a32(name)>`.
/// The hash is one-way, so reading a property *by name* requires hashing each
/// ontology-declared name and matching the resulting key.
fn prop_name_to_col(name: &str) -> String {
    let mut hash: u32 = 2166136261u32;
    for byte in name.bytes() {
        hash ^= byte as u32;
        hash = hash.wrapping_mul(16777619);
    }
    format!("col_{hash}")
}

// ── Public report types ───────────────────────────────────────────────────────

/// Something the exporter could not represent in RDF.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ExportIssue {
    /// Subject IRI (or label / relationship type) the issue concerns.
    pub subject: String,
    /// Actionable description.
    pub reason: String,
}

/// What [`export_data_with_report`] could not express in RDF.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct ExportReport {
    pub entities_exported: usize,
    pub relationships_exported: usize,
    pub triples_emitted: usize,
    /// Stored columns with no matching ontology-declared property name. The
    /// `col_<hash>` key cannot be reversed, so these cannot be named in RDF.
    pub orphan_properties: usize,
    /// Node labels present in the database with no matching ontology class.
    pub orphan_labels: Vec<String>,
    /// Relationship types present in the database with no matching relation.
    pub orphan_relation_types: Vec<String>,
    /// Edges whose source or target node no longer exists.
    pub dangling_edges: usize,
    /// Classes or entities whose name/IRI could not be turned into a valid
    /// RDF IRI (e.g. a class name containing a space). Skipped rather than
    /// failing the whole export — see `issues` for detail.
    pub unrepresentable_iris: usize,
    pub issues: Vec<ExportIssue>,
}

/// How [`import_data_turtle`] treats classes, relations, and properties that
/// the ontology does not already declare.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportStrategy {
    /// Reject unknown classes / relations / properties. The offending subject
    /// is skipped and reported; nothing is added to the ontology.
    Strict,
    /// Declare unknown classes, relations, and properties on the fly, deriving
    /// property types from the literal's XSD datatype and relation domain /
    /// range from the observed subject and object classes.
    AutoDeclare,
}

/// One subject (or triple) that was not imported, and why.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ImportSkip {
    /// The offending subject IRI.
    pub subject: String,
    /// Actionable reason, in the style of the ontology's other error messages.
    pub reason: String,
}

/// Outcome of [`import_data_turtle`].
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct ImportReport {
    pub entities_imported: usize,
    pub relationships_imported: usize,
    pub entities_skipped: usize,
    pub relationships_skipped: usize,
    /// Blank-node subjects or objects encountered. WS1 flags and skips them.
    pub blank_nodes_skipped: usize,
    pub classes_declared: usize,
    pub relations_declared: usize,
    pub properties_declared: usize,
    /// Every skip, with the offending subject IRI and an actionable reason.
    pub skips: Vec<ImportSkip>,
    /// Non-fatal notes (dropped language tags, parse errors on single triples).
    pub warnings: Vec<String>,
}

// ── Schema-derived lookup tables ──────────────────────────────────────────────

/// Everything the exporter and importer need to translate between ontology
/// names and IRIs, derived once from a single [`export_schema`] call.
struct SchemaIndex {
    /// class name → class IRI
    class_iri: HashMap<String, String>,
    /// relation name → relation IRI
    relation_iri: HashMap<String, String>,
    /// relation name → (domain class, range class)
    relation_domain_range: HashMap<String, (String, String)>,
    /// class name → (col key → property), including inherited properties.
    props_by_class: HashMap<String, HashMap<String, OntologyProperty>>,
    /// class name → (property name → property), including inherited.
    props_by_name: HashMap<String, HashMap<String, OntologyProperty>>,
    /// class IRI → class name (for import type resolution)
    class_by_iri: HashMap<String, String>,
    /// relation IRI → relation name (for import predicate resolution)
    relation_by_iri: HashMap<String, String>,
    /// property source IRI → property name (for import predicate resolution)
    property_by_iri: HashMap<String, String>,
}

impl SchemaIndex {
    fn build(snap: &SchemaSnapshot, base: &str) -> Self {
        let mut class_iri = HashMap::new();
        let mut class_by_iri = HashMap::new();
        for c in &snap.classes {
            let iri = match &c.iri {
                Some(s) if !s.is_empty() => s.clone(),
                // Deliberately NOT `so:{symbol_id}` (what the schema exporter
                // falls back to): symbol_id is a fresh UUID on every init, so a
                // symbol_id-derived IRI is not stable across a schema replay
                // and would break round-trip isomorphism. A name-derived IRI is
                // deterministic. Declare an explicit `iri` on the class to make
                // data.ttl and schema.ttl agree exactly.
                _ => format!("{base}/schema/{}", c.name),
            };
            class_by_iri.insert(iri.clone(), c.name.clone());
            class_iri.insert(c.name.clone(), iri);
        }

        let mut relation_iri = HashMap::new();
        let mut relation_by_iri = HashMap::new();
        let mut relation_domain_range = HashMap::new();
        for r in &snap.relations {
            let iri = match &r.iri {
                Some(s) if !s.is_empty() => s.clone(),
                _ => format!("{base}/schema/{}", r.name),
            };
            relation_by_iri.insert(iri.clone(), r.name.clone());
            relation_iri.insert(r.name.clone(), iri);
            relation_domain_range.insert(r.name.clone(), (r.domain.clone(), r.range.clone()));
        }

        // class symbol_id → name, and child → parent for inheritance walking.
        let name_by_sid: HashMap<&str, &str> = snap
            .classes
            .iter()
            .map(|c| (c.symbol_id.as_str(), c.name.as_str()))
            .collect();
        let parent_of: HashMap<&str, &str> = snap
            .subclass_edges
            .iter()
            .map(|(child, parent)| (child.as_str(), parent.as_str()))
            .collect();

        // Own properties per class symbol_id.
        let mut own: HashMap<&str, Vec<&OntologyProperty>> = HashMap::new();
        let mut property_by_iri = HashMap::new();
        for p in &snap.properties {
            own.entry(p.owner_symbol_id.as_str()).or_default().push(p);
            if let Some(src) = &p.source_iri {
                if !src.is_empty() {
                    property_by_iri.insert(src.clone(), p.name.clone());
                }
            }
        }

        let mut props_by_class = HashMap::new();
        let mut props_by_name = HashMap::new();
        for c in &snap.classes {
            // Walk from the most distant ancestor down to the class itself so
            // that a child-declared property overwrites an inherited one.
            let mut chain: Vec<&str> = Vec::new();
            let mut cursor = c.symbol_id.as_str();
            let mut guard = 0;
            loop {
                chain.push(cursor);
                match parent_of.get(cursor) {
                    Some(p) if guard < 32 && name_by_sid.contains_key(*p) => {
                        cursor = p;
                        guard += 1;
                    }
                    _ => break,
                }
            }
            chain.reverse();

            let mut by_name: HashMap<String, OntologyProperty> = HashMap::new();
            for sid in chain {
                for p in own.get(sid).into_iter().flatten() {
                    by_name.insert(p.name.clone(), (*p).clone());
                }
            }
            let by_col: HashMap<String, OntologyProperty> = by_name
                .values()
                .map(|p| (prop_name_to_col(&p.name), p.clone()))
                .collect();
            props_by_class.insert(c.name.clone(), by_col);
            props_by_name.insert(c.name.clone(), by_name);
        }

        Self {
            class_iri,
            relation_iri,
            relation_domain_range,
            props_by_class,
            props_by_name,
            class_by_iri,
            relation_by_iri,
            property_by_iri,
        }
    }
}

/// Trim a trailing `/` so `{base}/{Class}/{id}` never doubles the separator.
fn normalize_base(base_iri: &str) -> String {
    base_iri.trim_end_matches('/').to_string()
}

/// Extract the local name from an IRI (everything after the last `#` or `/`).
fn local_name(iri: &str) -> String {
    iri.rfind(['#', '/'])
        .map(|pos| iri[pos + 1..].to_owned())
        .unwrap_or_else(|| iri.to_owned())
}

fn named(iri: &str) -> Result<NamedNode, SoError> {
    NamedNode::new(iri).map_err(|e| {
        SoError::Storage(sparrowdb_common::Error::InvalidArgument(format!(
            "cannot build a valid IRI from '{iri}': {e}"
        )))
    })
}

// ── Value formatting ──────────────────────────────────────────────────────────

/// Format an `f64` as a valid `xsd:double` lexical form.
///
/// Rust's `f64::to_string` yields `1250.5` and `2` (not `2.0`); both are valid
/// `xsd:double` lexical forms and both re-parse to the same `f64`, so the
/// round-trip is stable.
fn fmt_f64(f: f64) -> String {
    if f.is_nan() {
        "NaN".to_string()
    } else if f.is_infinite() {
        if f > 0.0 { "INF" } else { "-INF" }.to_string()
    } else {
        f.to_string()
    }
}

/// Render a stored value as the RDF lexical form for its *declared* type.
///
/// The declared [`PropertyType`] governs, not the storage representation: a
/// `Bool` is stored as `Int64` 0/1 and a `Float64` is stored as a byte string,
/// so the runtime `ExecValue` variant alone cannot tell you the RDF datatype.
fn lexical_form(t: &PropertyType, v: &ExecValue) -> Option<String> {
    match (t, v) {
        (_, ExecValue::Null) => None,

        (PropertyType::Bool, ExecValue::Int64(n)) => {
            Some(if *n != 0 { "true" } else { "false" }.to_string())
        }
        (PropertyType::Bool, ExecValue::Bool(b)) => Some(b.to_string()),
        (PropertyType::Bool, ExecValue::String(s)) => match s.as_str() {
            "true" | "1" => Some("true".to_string()),
            "false" | "0" => Some("false".to_string()),
            _ => None,
        },

        (PropertyType::Int64, ExecValue::Int64(n)) => Some(n.to_string()),
        (PropertyType::Int64, ExecValue::String(s)) => s.parse::<i64>().ok().map(|n| n.to_string()),
        (PropertyType::Int64, ExecValue::Float64(f)) => Some((*f as i64).to_string()),

        (PropertyType::Float64, ExecValue::Float64(f)) => Some(fmt_f64(*f)),
        (PropertyType::Float64, ExecValue::String(s)) => s.parse::<f64>().ok().map(fmt_f64),
        (PropertyType::Float64, ExecValue::Int64(n)) => Some(fmt_f64(*n as f64)),

        (_, ExecValue::String(s)) => Some(s.clone()),
        (_, ExecValue::Int64(n)) => Some(n.to_string()),
        (_, ExecValue::Float64(f)) => Some(fmt_f64(*f)),
        (_, ExecValue::Bool(b)) => Some(b.to_string()),
        _ => None,
    }
}

/// Build the RDF object term for a declared property and its stored value.
fn object_term(t: &PropertyType, v: &ExecValue) -> Option<Term> {
    let lex = lexical_form(t, v)?;
    let dt = match t {
        PropertyType::Date => date_datatype_for(&lex),
        other => property_type_to_xsd(other),
    };
    let dt_node = NamedNode::new(dt).ok()?;
    Some(Term::Literal(Literal::new_typed_literal(lex, dt_node)))
}

// ── Export ────────────────────────────────────────────────────────────────────

/// Export every entity and relationship as Turtle.
///
/// Subjects, predicates, and datatypes are derived from the ontology; see the
/// module docs for IRI minting rules and the datatype table.
///
/// Note the signature takes `db` first, matching
/// [`crate::jsonld::export_json_ld`]; the spec writes it as
/// `export_data_turtle(base_iri)` but the database is not implicit anywhere
/// else in this crate.
///
/// # Errors
/// Returns `SoError` if a schema read fails or `base_iri` cannot form valid IRIs.
pub fn export_data_turtle(db: &GraphDb, base_iri: &str) -> Result<String, SoError> {
    Ok(export_data_with_report(db, base_iri)?.0)
}

/// Like [`export_data_turtle`], but also returns the [`ExportReport`] listing
/// everything that could not be represented (orphan columns, orphan labels,
/// dangling edges).
pub fn export_data_with_report(
    db: &GraphDb,
    base_iri: &str,
) -> Result<(String, ExportReport), SoError> {
    let (triples, report) = collect_data_triples(db, base_iri)?;

    let mut ser = oxttl::TurtleSerializer::new();
    for (prefix, ns) in [
        ("rdf", "http://www.w3.org/1999/02/22-rdf-syntax-ns#"),
        ("xsd", XSD_NS),
    ] {
        ser = ser
            .with_prefix(prefix, ns)
            .map_err(|e| invalid(format!("bad prefix {prefix}: {e}")))?;
    }

    let mut writer = ser.for_writer(Vec::new());
    for t in &triples {
        writer
            .serialize_triple(t.as_ref())
            .map_err(|e| invalid(format!("Turtle serialization failed: {e}")))?;
    }
    let bytes = writer
        .finish()
        .map_err(|e| invalid(format!("Turtle serialization failed: {e}")))?;
    let ttl = String::from_utf8(bytes)
        .map_err(|e| invalid(format!("Turtle output was not valid UTF-8: {e}")))?;

    Ok((ttl, report))
}

fn invalid(msg: String) -> SoError {
    SoError::Storage(sparrowdb_common::Error::InvalidArgument(msg))
}

/// Run a query, treating "unknown label"/"unknown relationship type" as empty.
fn execute_or_empty(db: &GraphDb, q: &str) -> Result<sparrowdb_execution::QueryResult, SoError> {
    match db.execute(q) {
        Ok(r) => Ok(r),
        Err(sparrowdb_common::Error::InvalidArgument(ref msg))
            if msg.contains("unknown label") || msg.contains("unknown relationship type") =>
        {
            Ok(sparrowdb_execution::QueryResult::empty(vec![]))
        }
        Err(e) => Err(SoError::Storage(e)),
    }
}

/// Build the deduplicated, deterministically ordered triple list for the data graph.
///
/// Triples are keyed by their N-Triples form in a `BTreeMap`, which dedupes
/// (RDF graphs are sets — a duplicate edge in storage must not become a
/// duplicate triple) and yields a stable order across runs.
fn collect_data_triples(
    db: &GraphDb,
    base_iri: &str,
) -> Result<(Vec<Triple>, ExportReport), SoError> {
    let base = normalize_base(base_iri);
    let snap = export_schema(db)?;
    let idx = SchemaIndex::build(&snap, &base);

    let mut report = ExportReport::default();
    let mut out: BTreeMap<String, Triple> = BTreeMap::new();

    let rdf_type = named(RDF_TYPE)?;
    let iri_col = prop_name_to_col(SOURCE_IRI_KEY);
    let source_label_col = prop_name_to_col(SOURCE_LABEL_KEY);

    // node id → subject IRI, for edge endpoints. Also doubles as the "this node
    // is live" set used to filter dangling edges.
    let mut subject_by_id: HashMap<i64, NamedNode> = HashMap::new();

    // ── Entities ──────────────────────────────────────────────────────────────
    let mut labels = db.labels().map_err(SoError::Storage)?;
    labels.sort();
    for label in &labels {
        if label.starts_with(SO_NAMESPACE) {
            continue;
        }
        let Some(class_iri) = idx.class_iri.get(label) else {
            report.orphan_labels.push(label.clone());
            report.issues.push(ExportIssue {
                subject: label.clone(),
                reason: format!(
                    "Label '{label}' is not a declared ontology class; its nodes were not \
                     exported. Declare it with define_class, or remove the nodes."
                ),
            });
            continue;
        };
        let class_node = match named(class_iri) {
            Ok(n) => n,
            Err(e) => {
                report.unrepresentable_iris += 1;
                report.issues.push(ExportIssue {
                    subject: label.clone(),
                    reason: format!(
                        "class '{label}' has an IRI that could not be represented in RDF \
                         ({e}); its nodes were not exported. Set a valid `iri` on the class \
                         (see define_class), or rename it to avoid characters that are \
                         invalid in an IRI."
                    ),
                });
                continue;
            }
        };
        let empty = HashMap::new();
        let cols = idx.props_by_class.get(label).unwrap_or(&empty);

        let q = format!("MATCH (n:{label}) RETURN id(n), n");
        let result = execute_or_empty(db, &q)?;
        for row in &result.rows {
            let Some(ExecValue::Int64(node_id)) = row.first() else {
                continue;
            };
            let props: &Vec<(String, ExecValue)> = match row.get(1) {
                Some(ExecValue::Map(m)) => m,
                _ => continue,
            };

            // Subject IRI: stored __so_iri wins over minting.
            let stored_iri = props.iter().find_map(|(k, v)| {
                if k == &iri_col {
                    if let ExecValue::String(s) = v {
                        if !s.is_empty() {
                            return Some(s.clone());
                        }
                    }
                }
                None
            });
            let subject_iri = stored_iri.unwrap_or_else(|| format!("{base}/{label}/{node_id}"));
            let subject = match named(&subject_iri) {
                Ok(s) => s,
                Err(e) => {
                    report.unrepresentable_iris += 1;
                    report.issues.push(ExportIssue {
                        subject: subject_iri.clone(),
                        reason: format!(
                            "node id {node_id} on label '{label}' has an IRI that could not \
                             be represented in RDF ({e}); it was not exported. This node's \
                             edges will be reported as dangling."
                        ),
                    });
                    continue;
                }
            };

            subject_by_id.insert(*node_id, subject.clone());
            report.entities_exported += 1;

            insert_triple(
                &mut out,
                Triple::new(subject.clone(), rdf_type.clone(), class_node.clone()),
            );

            for (col_key, value) in props {
                if col_key == &iri_col || col_key == &source_label_col {
                    // The subject IRI and the alias-provenance marker are
                    // bookkeeping, not data properties.
                    continue;
                }
                let Some(prop) = cols.get(col_key) else {
                    // col_<hash> is one-way: with no ontology declaration whose
                    // name hashes to this key, the property cannot be named.
                    report.orphan_properties += 1;
                    report.issues.push(ExportIssue {
                        subject: subject_iri.clone(),
                        reason: format!(
                            "Stored column '{col_key}' has no ontology-declared property on \
                             class '{label}'. Property names hash one-way to col_<fnv1a32>, so \
                             this value cannot be named in RDF. Declare it with \
                             add_property(owner='{label}', …) and re-export."
                        ),
                    });
                    continue;
                };
                let Some(obj) = object_term(&prop.datatype, value) else {
                    continue; // NULL or an unrepresentable value: nothing to emit.
                };
                let pred_iri = property_predicate_iri(&base, prop);
                insert_triple(
                    &mut out,
                    Triple::new(subject.clone(), named(&pred_iri)?, obj),
                );
            }
        }
    }

    // ── Relationships ─────────────────────────────────────────────────────────
    let mut rel_types = db.relationship_types().map_err(SoError::Storage)?;
    rel_types.sort();
    for rel in &rel_types {
        if rel.starts_with(SO_NAMESPACE) {
            continue;
        }
        let Some(rel_iri) = idx.relation_iri.get(rel) else {
            report.orphan_relation_types.push(rel.clone());
            report.issues.push(ExportIssue {
                subject: rel.clone(),
                reason: format!(
                    "Relationship type '{rel}' is not a declared ontology relation; its edges \
                     were not exported. Declare it with define_relation."
                ),
            });
            continue;
        };
        let pred = named(rel_iri)?;

        let q = format!("MATCH (a)-[r:{rel}]->(b) RETURN id(a), id(b)");
        let result = execute_or_empty(db, &q)?;
        for row in &result.rows {
            let (Some(ExecValue::Int64(src)), Some(ExecValue::Int64(dst))) =
                (row.first(), row.get(1))
            else {
                continue;
            };
            // Deleting a node through Cypher `MATCH … DELETE` leaves its edges
            // behind (SparrowDB 0.1.22), so an edge endpoint may name a node
            // that no longer exists. Emitting a triple for it would assert
            // facts about a deleted entity.
            let (Some(s), Some(o)) = (subject_by_id.get(src), subject_by_id.get(dst)) else {
                report.dangling_edges += 1;
                report.issues.push(ExportIssue {
                    subject: format!("{src} -[{rel}]-> {dst}"),
                    reason: "Edge references a node that no longer exists; skipped. This is \
                             left behind by `MATCH (n:Label) DELETE n`, which does not remove \
                             incident edges."
                        .to_string(),
                });
                continue;
            };
            report.relationships_exported += 1;
            insert_triple(
                &mut out,
                Triple::new(s.clone(), pred.clone(), Term::NamedNode(o.clone())),
            );
        }
    }

    let triples: Vec<Triple> = out.into_values().collect();
    report.triples_emitted = triples.len();
    Ok((triples, report))
}

fn insert_triple(out: &mut BTreeMap<String, Triple>, t: Triple) {
    out.insert(t.to_string(), t);
}

/// Predicate IRI for a data property: its `source_iri` if it was imported from
/// an external vocabulary, otherwise minted from the property *name*.
///
/// Minting from the name (not the owning class) is deliberate: it mirrors how
/// RDF vocabularies work — `foaf:name` is one predicate regardless of subject
/// class — and it lets import resolve a predicate without knowing the class first.
fn property_predicate_iri(base: &str, prop: &OntologyProperty) -> String {
    match &prop.source_iri {
        Some(s) if !s.is_empty() => s.clone(),
        _ => format!("{base}/schema/{}", prop.name),
    }
}

// ── JSON-LD export ────────────────────────────────────────────────────────────

/// Export the same graph as [`export_data_turtle`] in JSON-LD 1.1.
///
/// The `@context` is derived from the ontology: every declared property becomes
/// a term with an `@type` datatype coercion, and every relation becomes a term
/// with `"@type": "@id"`.
///
/// Literal values are emitted as JSON **strings** and typed by the context's
/// coercion rules rather than by native JSON types. This guarantees the
/// JSON-LD graph is triple-for-triple identical to the Turtle graph — a JSON
/// number cannot express `xsd:integer` vs `xsd:double` unambiguously.
///
/// # Errors
/// Returns `SoError` if a schema read fails or `base_iri` cannot form valid IRIs.
pub fn export_data_json_ld(db: &GraphDb, base_iri: &str) -> Result<JsVal, SoError> {
    let base = normalize_base(base_iri);
    let snap = export_schema(db)?;
    let idx = SchemaIndex::build(&snap, &base);
    let (triples, _) = collect_data_triples(db, &base)?;

    // ── @context ──────────────────────────────────────────────────────────────
    let mut context = JsMap::new();
    context.insert("xsd".into(), json!(XSD_NS));

    // Relations: object properties.
    let mut relation_term_by_iri: HashMap<String, String> = HashMap::new();
    for (name, iri) in &idx.relation_iri {
        context.insert(name.clone(), json!({ "@id": iri, "@type": "@id" }));
        relation_term_by_iri.insert(iri.clone(), name.clone());
    }

    // Data properties: one term per property name, with datatype coercion.
    // A name declared on several classes must agree on its type to be given a
    // context term; otherwise the full IRI is used inline.
    let mut prop_term_by_iri: HashMap<String, String> = HashMap::new();
    let mut seen_type: HashMap<String, &'static str> = HashMap::new();
    let mut conflicting: HashSet<String> = HashSet::new();
    for p in &snap.properties {
        let iri = property_predicate_iri(&base, p);
        let xsd = property_type_to_xsd(&p.datatype);
        match seen_type.get(&p.name) {
            Some(prev) if *prev != xsd => {
                conflicting.insert(p.name.clone());
            }
            _ => {
                seen_type.insert(p.name.clone(), xsd);
            }
        }
        prop_term_by_iri.insert(iri, p.name.clone());
    }
    for p in &snap.properties {
        if conflicting.contains(&p.name) || relation_term_by_iri.contains_key(&p.name) {
            prop_term_by_iri.remove(&property_predicate_iri(&base, p));
            continue;
        }
        let iri = property_predicate_iri(&base, p);
        let xsd = property_type_to_xsd(&p.datatype);
        // Two cases get no `@type` coercion on the term:
        //
        // - Date, because the value decides between xsd:date and xsd:dateTime,
        //   so the datatype rides on each literal instead.
        // - xsd:string, because RDF 1.1 makes xsd:string the type of a plain
        //   literal. Turtle writes `"Acme Corp"`; coercing the JSON-LD term to
        //   xsd:string makes some processors (rdflib among them) produce a
        //   *typed* literal that no longer compares equal to the plain one, and
        //   the two serializations stop being the same graph.
        if p.datatype == PropertyType::Date || xsd == XSD_STRING {
            context.insert(p.name.clone(), json!({ "@id": iri }));
        } else {
            context.insert(p.name.clone(), json!({ "@id": iri, "@type": xsd }));
        }
    }

    // ── @graph ────────────────────────────────────────────────────────────────
    // Group triples by subject, preserving the deterministic order they arrive in.
    let mut order: Vec<String> = Vec::new();
    let mut by_subject: HashMap<String, JsMap<String, JsVal>> = HashMap::new();

    for t in &triples {
        let subj = t.subject.to_string();
        let subj = subj.trim_matches(|c| c == '<' || c == '>').to_string();
        if !by_subject.contains_key(&subj) {
            order.push(subj.clone());
            let mut m = JsMap::new();
            m.insert("@id".into(), json!(subj));
            by_subject.insert(subj.clone(), m);
        }
        let entry = by_subject.get_mut(&subj).expect("just inserted");
        let pred = t.predicate.as_str();

        if pred == RDF_TYPE {
            if let Term::NamedNode(n) = &t.object {
                entry.insert("@type".into(), json!(n.as_str()));
            }
            continue;
        }

        let (key, value) = match &t.object {
            Term::NamedNode(n) => {
                let key = relation_term_by_iri
                    .get(pred)
                    .cloned()
                    .unwrap_or_else(|| pred.to_string());
                (key, json!({ "@id": n.as_str() }))
            }
            Term::Literal(lit) => {
                let key = prop_term_by_iri
                    .get(pred)
                    .cloned()
                    .unwrap_or_else(|| pred.to_string());
                let dt = lit.datatype().as_str();
                // Terms carry their coercion in @context. Dates (and any
                // predicate that had to fall back to a full IRI) need the
                // datatype spelled out on the value itself.
                let needs_explicit = !context
                    .get(&key)
                    .and_then(|t| t.get("@type"))
                    .map(|t| t == dt)
                    .unwrap_or(false);
                if needs_explicit && dt != XSD_STRING {
                    (key, json!({ "@value": lit.value(), "@type": dt }))
                } else {
                    (key, json!(lit.value()))
                }
            }
            // The exporter never emits blank nodes.
            Term::BlankNode(_) => continue,
        };

        match entry.get_mut(&key) {
            Some(JsVal::Array(arr)) => arr.push(value),
            Some(existing) => {
                let prev = existing.clone();
                *existing = JsVal::Array(vec![prev, value]);
            }
            None => {
                entry.insert(key, value);
            }
        }
    }

    let graph: Vec<JsVal> = order
        .into_iter()
        .filter_map(|s| by_subject.remove(&s).map(JsVal::Object))
        .collect();

    Ok(json!({ "@context": JsVal::Object(context), "@graph": graph }))
}

// ── Import ────────────────────────────────────────────────────────────────────

fn sv(s: &str) -> StoreValue {
    StoreValue::Bytes(s.as_bytes().to_vec())
}

/// Mirror of the MCP layer's `property_value_to_store`, so that entities written
/// by import read back byte-identically to entities written by `create_entity`.
fn property_value_to_store(v: &PropertyValue) -> Option<StoreValue> {
    match v {
        PropertyValue::String(s) => Some(StoreValue::Bytes(s.as_bytes().to_vec())),
        PropertyValue::Int64(n) => Some(StoreValue::Int64(*n)),
        PropertyValue::Float64(f) => Some(StoreValue::Bytes(fmt_f64(*f).into_bytes())),
        PropertyValue::Bool(b) => Some(StoreValue::Int64(if *b { 1 } else { 0 })),
        PropertyValue::Null => None,
    }
}

/// Coerce a literal to the declared property type.
fn literal_to_property_value(t: &PropertyType, lit: &Literal) -> Result<PropertyValue, String> {
    let s = lit.value();
    match t {
        PropertyType::String | PropertyType::Date | PropertyType::Variant => {
            Ok(PropertyValue::String(s.to_string()))
        }
        PropertyType::Int64 => s
            .parse::<i64>()
            .map(PropertyValue::Int64)
            .map_err(|_| format!("expected an integer for a declared int64 property, got '{s}'")),
        PropertyType::Float64 => s
            .parse::<f64>()
            .map(PropertyValue::Float64)
            .map_err(|_| format!("expected a number for a declared float64 property, got '{s}'")),
        PropertyType::Bool => match s {
            "true" | "1" => Ok(PropertyValue::Bool(true)),
            "false" | "0" => Ok(PropertyValue::Bool(false)),
            other => Err(format!(
                "expected 'true' or 'false' for a declared bool property, got '{other}'"
            )),
        },
    }
}

/// Everything gathered about one subject during the parse pass.
struct SubjectData {
    iri: String,
    type_iri: Option<String>,
    literals: Vec<(String, Literal)>,
    links: Vec<(String, String)>,
}

/// Import entities and relationships from a Turtle document.
///
/// Subjects whose `rdf:type` matches a known class (by IRI, or by the IRI's
/// local name resolved through the alias table) become entities written through
/// the ontology's validated write path. The subject IRI is persisted on each
/// entity so a later export reproduces it exactly.
///
/// Nothing is dropped silently: every skipped subject or triple is recorded in
/// [`ImportReport::skips`] with the offending subject IRI and an actionable reason.
///
/// # Errors
/// Returns `SoError` only for storage-level failures that affect the whole
/// import. Per-subject problems are collected in the report.
pub fn import_data_turtle(
    db: &GraphDb,
    turtle: &str,
    strategy: ImportStrategy,
) -> Result<ImportReport, SoError> {
    let mut report = ImportReport::default();

    // ── Parse ─────────────────────────────────────────────────────────────────
    // BTreeMap keeps subject processing order deterministic.
    let mut subjects: BTreeMap<String, SubjectData> = BTreeMap::new();

    for result in oxttl::TurtleParser::new().for_slice(turtle.as_bytes()) {
        let triple = match result {
            Ok(t) => t,
            Err(e) => {
                report.warnings.push(format!("Parse error: {e}"));
                continue;
            }
        };

        let subject_iri = match &triple.subject {
            oxrdf::NamedOrBlankNode::NamedNode(n) => n.as_str().to_string(),
            oxrdf::NamedOrBlankNode::BlankNode(_) => {
                // WS1 non-goal: blank nodes are flagged and skipped, never
                // silently dropped (Vault-LD §5.6 spirit).
                report.blank_nodes_skipped += 1;
                continue;
            }
        };

        let entry = subjects
            .entry(subject_iri.clone())
            .or_insert_with(|| SubjectData {
                iri: subject_iri.clone(),
                type_iri: None,
                literals: Vec::new(),
                links: Vec::new(),
            });

        let pred = triple.predicate.as_str().to_string();
        match &triple.object {
            Term::NamedNode(n) if pred == RDF_TYPE => {
                entry.type_iri = Some(n.as_str().to_string());
            }
            Term::NamedNode(n) => entry.links.push((pred, n.as_str().to_string())),
            Term::Literal(lit) => {
                if lit.language().is_some() {
                    report.warnings.push(format!(
                        "Language tag on <{subject_iri}> <{pred}> was dropped; SparrowOntology \
                         v1 stores plain strings."
                    ));
                }
                entry.literals.push((pred, lit.clone()));
            }
            Term::BlankNode(_) => {
                report.blank_nodes_skipped += 1;
            }
        }
    }

    // ── Resolve classes ───────────────────────────────────────────────────────
    let mut idx = SchemaIndex::build(&export_schema(db)?, "");

    // subject IRI → resolved canonical class name
    let mut class_of: BTreeMap<String, String> = BTreeMap::new();

    for (iri, data) in &subjects {
        let Some(type_iri) = &data.type_iri else {
            report.entities_skipped += 1;
            report.skips.push(ImportSkip {
                subject: iri.clone(),
                reason: "No rdf:type. Every subject needs an rdf:type naming a known class."
                    .to_string(),
            });
            continue;
        };

        if let Some(name) = idx.class_by_iri.get(type_iri) {
            class_of.insert(iri.clone(), name.clone());
            continue;
        }
        let local = local_name(type_iri);
        if let Ok(r) = resolve(db, &local, AliasKind::Class) {
            class_of.insert(iri.clone(), r.canonical_name);
            continue;
        }

        match strategy {
            ImportStrategy::Strict => {
                report.entities_skipped += 1;
                report.skips.push(ImportSkip {
                    subject: iri.clone(),
                    reason: format!(
                        "Unknown class <{type_iri}>. Valid: {:?}. Declare it with define_class, \
                         or re-run with ImportStrategy::AutoDeclare.",
                        sorted_keys(&idx.class_iri)
                    ),
                });
            }
            ImportStrategy::AutoDeclare => {
                write_class_node(db, &local, "", type_iri)?;
                report.classes_declared += 1;
                class_of.insert(iri.clone(), local);
            }
        }
    }

    if report.classes_declared > 0 {
        idx = SchemaIndex::build(&export_schema(db)?, "");
    }

    // ── Auto-declare missing data properties ──────────────────────────────────
    if strategy == ImportStrategy::AutoDeclare {
        let mut added = false;
        for (iri, data) in &subjects {
            let Some(class) = class_of.get(iri) else {
                continue;
            };
            for (pred, lit) in &data.literals {
                let name = idx
                    .property_by_iri
                    .get(pred)
                    .cloned()
                    .unwrap_or_else(|| local_name(pred));
                let declared = idx
                    .props_by_name
                    .get(class)
                    .map(|m| m.contains_key(&name))
                    .unwrap_or(false);
                if declared || name.starts_with("__so_") {
                    continue;
                }
                let ty = xsd_to_type_str(lit.datatype().as_str());
                crate::init::add_property(
                    db,
                    class,
                    &name,
                    ty,
                    false,
                    false,
                    None,
                    None,
                    Some(pred),
                )?;
                report.properties_declared += 1;
                added = true;
            }
        }
        if added {
            idx = SchemaIndex::build(&export_schema(db)?, "");
        }
    }

    // ── Auto-declare missing relations ────────────────────────────────────────
    if strategy == ImportStrategy::AutoDeclare {
        let mut added = false;
        for (iri, data) in &subjects {
            let Some(src_class) = class_of.get(iri) else {
                continue;
            };
            for (pred, target) in &data.links {
                if idx.relation_by_iri.contains_key(pred) {
                    continue;
                }
                let name = local_name(pred);
                if idx.relation_iri.contains_key(&name)
                    || idx.property_by_iri.contains_key(pred)
                    || idx
                        .props_by_name
                        .get(src_class)
                        .map(|m| m.contains_key(&name))
                        .unwrap_or(false)
                {
                    continue;
                }
                let Some(dst_class) = class_of.get(target) else {
                    continue; // handled as a skip in the relationship pass
                };
                write_relation_node(db, &name, "", pred, Some(src_class), Some(dst_class))?;
                report.relations_declared += 1;
                added = true;
            }
        }
        if added {
            idx = SchemaIndex::build(&export_schema(db)?, "");
        }
    }

    // ── Create entities ───────────────────────────────────────────────────────
    let mut node_of: HashMap<String, (NodeId, String)> = HashMap::new();

    for (iri, data) in &subjects {
        let Some(class) = class_of.get(iri) else {
            continue; // already reported
        };
        let empty = HashMap::new();
        let declared = idx.props_by_name.get(class).unwrap_or(&empty);

        let mut props: HashMap<String, PropertyValue> = HashMap::new();
        let mut failed: Option<String> = None;

        for (pred, lit) in &data.literals {
            let name = idx
                .property_by_iri
                .get(pred)
                .cloned()
                .unwrap_or_else(|| local_name(pred));
            let Some(prop) = declared.get(&name) else {
                failed = Some(format!(
                    "Unknown property '{name}' (from <{pred}>) on class '{class}'. Valid: {:?}. \
                     Declare it with add_property(owner='{class}', name='{name}'), or re-run \
                     with ImportStrategy::AutoDeclare.",
                    sorted_keys(declared)
                ));
                break;
            };
            match literal_to_property_value(&prop.datatype, lit) {
                Ok(v) => {
                    props.insert(name, v);
                }
                Err(e) => {
                    failed = Some(format!("Property '{name}': {e}"));
                    break;
                }
            }
        }

        if let Some(reason) = failed {
            report.entities_skipped += 1;
            report.skips.push(ImportSkip {
                subject: iri.clone(),
                reason,
            });
            continue;
        }

        // Validated write path — same rules create_entity enforces.
        if let Err(e) = ValidationContext::new(db).validate_entity(class, &props, true) {
            report.entities_skipped += 1;
            report.skips.push(ImportSkip {
                subject: iri.clone(),
                reason: e.to_string(),
            });
            continue;
        }

        let mut store: HashMap<String, StoreValue> = props
            .iter()
            .filter_map(|(k, v)| property_value_to_store(v).map(|s| (k.clone(), s)))
            .collect();
        // Persist the subject IRI so the next export reproduces it instead of
        // minting a new one from the freshly assigned node id.
        store.insert(SOURCE_IRI_KEY.to_string(), sv(&data.iri));

        let mut tx = db.begin_write()?;
        let nid = tx.merge_node(class, store)?;
        tx.commit()?;

        node_of.insert(iri.clone(), (nid, class.clone()));
        report.entities_imported += 1;
    }

    // ── Create relationships ──────────────────────────────────────────────────
    for (iri, data) in &subjects {
        let Some((src_id, src_class)) = node_of.get(iri) else {
            continue;
        };
        for (pred, target) in &data.links {
            let rel_name = idx
                .relation_by_iri
                .get(pred)
                .cloned()
                .unwrap_or_else(|| local_name(pred));

            if !idx.relation_iri.contains_key(&rel_name) {
                report.relationships_skipped += 1;
                report.skips.push(ImportSkip {
                    subject: iri.clone(),
                    reason: format!(
                        "Unknown relation '{rel_name}' (from <{pred}>). Valid: {:?}. Declare it \
                         with define_relation, or re-run with ImportStrategy::AutoDeclare.",
                        sorted_keys(&idx.relation_iri)
                    ),
                });
                continue;
            }

            let Some((dst_id, dst_class)) = node_of.get(target) else {
                report.relationships_skipped += 1;
                report.skips.push(ImportSkip {
                    subject: iri.clone(),
                    reason: format!(
                        "Relation '{rel_name}' points at <{target}>, which was not imported as \
                         an entity (no rdf:type, or it was itself skipped)."
                    ),
                });
                continue;
            };

            if let Err(e) =
                ValidationContext::new(db).validate_relationship(&rel_name, src_class, dst_class)
            {
                report.relationships_skipped += 1;
                report.skips.push(ImportSkip {
                    subject: iri.clone(),
                    reason: e.to_string(),
                });
                continue;
            }

            let mut tx = db.begin_write()?;
            tx.create_edge(*src_id, *dst_id, &rel_name, HashMap::new())?;
            tx.commit()?;
            report.relationships_imported += 1;
        }
    }

    let _ = &idx.relation_domain_range; // kept for future range-narrowing checks
    Ok(report)
}

fn sorted_keys<V>(m: &HashMap<String, V>) -> Vec<String> {
    let mut v: Vec<String> = m.keys().cloned().collect();
    v.sort();
    v
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot::property_type_from_str;

    #[test]
    fn base_iri_trailing_slash_is_normalized() {
        assert_eq!(normalize_base("https://ex.org/kb/"), "https://ex.org/kb");
        assert_eq!(normalize_base("https://ex.org/kb"), "https://ex.org/kb");
    }

    #[test]
    fn local_name_handles_hash_and_slash() {
        assert_eq!(local_name("http://xmlns.com/foaf/0.1/name"), "name");
        assert_eq!(local_name("http://www.w3.org/2002/07/owl#Class"), "Class");
        assert_eq!(local_name("bare"), "bare");
    }

    /// The `col_` reverse table only works if this hash matches the engine's.
    /// Derived by hand: FNV-1a 32-bit over the ASCII bytes of "name", starting
    /// from offset basis 2166136261 with prime 16777619. Probe of a live
    /// SparrowDB (`MATCH (n:Person) RETURN n` on a node with name="Ada")
    /// returned the key `col_2369371622`.
    #[test]
    fn fnv1a32_matches_the_engine_column_keys() {
        assert_eq!(prop_name_to_col("name"), "col_2369371622");
    }

    /// Anti-drift guard for the datatype table.
    ///
    /// `property_type_to_xsd` (here) and `xsd_to_type_str` (turtle_import) live
    /// in different modules and would otherwise be free to drift apart. Every
    /// `PropertyType` must survive export → XSD IRI → import → `PropertyType`
    /// unchanged, except `Variant`, which has no XSD equivalent and is
    /// documented as collapsing to `String`.
    ///
    /// Expected values derived by hand from the mapping table in the module
    /// docs, not captured from program output.
    #[test]
    fn datatype_mapping_is_a_fixed_point() {
        let cases = [
            (PropertyType::String, XSD_STRING, PropertyType::String),
            (PropertyType::Int64, XSD_INTEGER, PropertyType::Int64),
            (PropertyType::Float64, XSD_DOUBLE, PropertyType::Float64),
            (PropertyType::Bool, XSD_BOOLEAN, PropertyType::Bool),
            (PropertyType::Date, XSD_DATE, PropertyType::Date),
            // Documented lossy edge: Variant has no XSD counterpart.
            (PropertyType::Variant, XSD_STRING, PropertyType::String),
        ];
        for (from, expected_xsd, expected_back) in cases {
            let xsd = property_type_to_xsd(&from);
            assert_eq!(
                xsd, expected_xsd,
                "{from:?} should export as {expected_xsd}"
            );
            let back = property_type_from_str(xsd_to_type_str(xsd));
            assert_eq!(
                back, expected_back,
                "{from:?} -> {xsd} should re-import as {expected_back:?}"
            );
        }
    }

    /// Every XSD datatype the importer accepts for a numeric/boolean/date type
    /// must map onto the type this module exports for. Hand-derived from
    /// `xsd_to_type_str`'s match arms.
    #[test]
    fn import_accepts_the_wider_xsd_families() {
        for local in [
            "integer",
            "int",
            "long",
            "short",
            "byte",
            "nonNegativeInteger",
            "positiveInteger",
            "unsignedInt",
        ] {
            assert_eq!(
                xsd_to_type_str(&format!("{XSD_NS}{local}")),
                "int64",
                "xsd:{local} should import as int64"
            );
        }
        for local in ["decimal", "float", "double"] {
            assert_eq!(xsd_to_type_str(&format!("{XSD_NS}{local}")), "float64");
        }
        assert_eq!(xsd_to_type_str(XSD_BOOLEAN), "bool");
        assert_eq!(xsd_to_type_str(XSD_DATE), "date");
        assert_eq!(xsd_to_type_str(XSD_DATE_TIME), "date");
        // xsd:time has no calendar component and no Sparrow equivalent.
        assert_eq!(xsd_to_type_str(&format!("{XSD_NS}time")), "string");
    }

    /// A `Date` property holding a dateTime lexical form must be tagged
    /// `xsd:dateTime`, not `xsd:date` — `"2026-03-01T09:30:00Z"^^xsd:date` is
    /// an ill-typed literal.
    #[test]
    fn date_datatype_follows_the_lexical_form() {
        assert_eq!(date_datatype_for("2026-03-01"), XSD_DATE);
        assert_eq!(date_datatype_for("2026-03-01T09:30:00Z"), XSD_DATE_TIME);
    }

    /// Bools are stored as `Int64` 0/1 and floats as byte strings, so the
    /// declared type — not the storage variant — must drive the lexical form.
    #[test]
    fn lexical_form_follows_the_declared_type_not_the_storage_type() {
        assert_eq!(
            lexical_form(&PropertyType::Bool, &ExecValue::Int64(1)).unwrap(),
            "true"
        );
        assert_eq!(
            lexical_form(&PropertyType::Bool, &ExecValue::Int64(0)).unwrap(),
            "false"
        );
        assert_eq!(
            lexical_form(
                &PropertyType::Float64,
                &ExecValue::String("1250.5".to_string())
            )
            .unwrap(),
            "1250.5"
        );
        assert_eq!(
            lexical_form(&PropertyType::Int64, &ExecValue::Int64(7)).unwrap(),
            "7"
        );
        assert_eq!(lexical_form(&PropertyType::String, &ExecValue::Null), None);
    }

    /// Whole-number doubles render without a trailing `.0`; both forms are
    /// valid `xsd:double` and re-parse to the same value, so the round-trip is
    /// byte-stable.
    #[test]
    fn f64_formatting_round_trips() {
        for v in [1250.5f64, 2.0, -0.25, 1e10] {
            let s = fmt_f64(v);
            assert_eq!(s.parse::<f64>().unwrap(), v, "{s} should re-parse to {v}");
        }
        assert_eq!(fmt_f64(2.0), "2");
    }
}
