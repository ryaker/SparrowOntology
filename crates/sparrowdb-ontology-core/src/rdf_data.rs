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
//! # Re-import is not idempotent
//!
//! [`import_data_turtle`] is safe to call once against an empty (or
//! never-imported) database. It is **not** safe to call a second time
//! against a database that already holds the data and expect a no-op:
//!
//! - **Entities.** The write path dedups an incoming node against an
//!   existing one only when *every* stored column matches exactly — not on
//!   [`SOURCE_IRI_KEY`](crate::namespace::SOURCE_IRI_KEY) alone. If even one
//!   property value changed since the first import (or the ontology gained a
//!   property), re-importing the same subject IRI creates a **second** node
//!   carrying that IRI instead of updating the first.
//! - **Relationships.** Edge creation has no dedup at all. Re-importing
//!   identical Turtle into a database that already has it **doubles** every
//!   relationship between the same pair of entities.
//!
//! Neither case is visible in [`ImportReport`] — there is no "this
//! duplicated an existing entity/edge" counter, only counts for what was
//! newly created. A caller that needs incremental sync (as opposed to a
//! one-shot import into an empty database) must wipe the target data first,
//! or de-duplicate the Turtle against current database state before calling
//! [`import_data_turtle`]. This is a design gap, not a bug fixable from this
//! function alone — tracked as a follow-up rather than addressed here.
//!
//! # Non-goals (WS1)
//!
//! No SPARQL engine, no named graphs, no reasoning, and no blank-node
//! preservation on import — blank nodes are flagged in the report and skipped,
//! per the spirit of Vault-LD §5.6.

use std::collections::{BTreeMap, HashMap, HashSet};

use oxrdf::{Literal, NamedNode, Term, Triple};
use serde_json::{json, Map as JsMap, Value as JsVal};
use sparrowdb::{GraphDb, ReadTx};
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

/// Backtick-quote a label or relationship-type for safe interpolation into a
/// generated Cypher query (`MATCH (n:{ident}) ...`).
///
/// Labels and relationship types reach this module from `db.labels()` /
/// `db.relationship_types()` — names that were, at write time, accepted
/// verbatim by `merge_node`/`create_edge` with no character validation (they
/// are opaque catalog keys there, not Cypher text). By the time they get
/// here they are about to be spliced into a `format!`-built query string, so
/// an unquoted name containing e.g. `)` or a space would corrupt the query.
/// SparrowDB's Cypher lexer supports backtick-quoted identifiers for exactly
/// this case, but has no escape for an embedded backtick — so a name that
/// itself contains a backtick cannot be made safe this way and is rejected.
fn cypher_ident(name: &str) -> Result<String, SoError> {
    if name.contains('`') {
        return Err(SoError::Storage(sparrowdb_common::Error::InvalidArgument(
            format!("'{name}' contains a backtick, which cannot be safely quoted in Cypher"),
        )));
    }
    Ok(format!("`{name}`"))
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

/// Run a query through a shared [`ReadTx`], treating "unknown label"/"unknown
/// relationship type" as empty.
///
/// NOTE: despite `ReadTx`'s name and its `snapshot_txn_id` field, this does
/// **not** give the query a point-in-time view of *structural* state (nodes,
/// edges). See the "Snapshot isolation" section on [`collect_data_triples`]
/// for what is and is not actually guaranteed here.
fn execute_or_empty(tx: &ReadTx, q: &str) -> Result<sparrowdb_execution::QueryResult, SoError> {
    match tx.query(q) {
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
///
/// ## Snapshot isolation — NOT closed, despite the shared [`ReadTx`]
///
/// The entity pass and the relationship pass each issue one query per
/// label/relation-type. Without a shared snapshot, a write committed between
/// those queries could be visible to one pass and not the other — e.g. a node
/// created after the entity pass scanned its label, then linked by an edge the
/// relationship pass *does* see. `subject_by_id` would have no entry for that
/// node, and the edge would be misreported as dangling even though it is live.
///
/// This function opens one [`ReadTx`] and routes every query through it,
/// which looks like the fix for that race. **It is not**, on the pinned
/// engine (SparrowDB `903ea739` / 0.1.27, tracked upstream as SparrowDB
/// #533): `ReadTx::query()` only pins *property-value* reads (via the MVCC
/// version chain, consulted by `ReadTx::get_node`, which this module does
/// not call). Structural state — which nodes and edges exist — is read
/// fresh from disk on every call, live, regardless of `snapshot_txn_id`.
/// This is SparrowDB's own documented behavior (see `readtx_query.rs`
/// module docs in that crate) and was confirmed empirically here: a
/// `ReadTx` opened before a `CREATE`, then queried after that `CREATE`
/// commits on the same handle, sees the new row (2 rows observed, not 1).
///
/// Concretely: the exact race described above — a node created and linked
/// between the entity and relationship passes — is **still possible**. A
/// shared `ReadTx` costs nothing and does at least pin property-value reads
/// consistently, and positions this code to gain real structural isolation
/// for free if `ReadTx::query()` ever gains it, so it is kept. But nothing
/// in `sparrowdb-ontology-core` can close this gap alone; it needs either an
/// engine-level fix upstream, or this crate holding SparrowDB's exclusive
/// single-writer lock (`GraphDb::begin_write`) for the export's duration as a
/// coarser, intra-process-only substitute — a design decision, not made
/// here. `dangling_edges` in [`ExportReport`] is the symptom a caller would
/// see if this race is hit.
fn collect_data_triples(
    db: &GraphDb,
    base_iri: &str,
) -> Result<(Vec<Triple>, ExportReport), SoError> {
    let base = normalize_base(base_iri);
    let snap = export_schema(db)?;
    let idx = SchemaIndex::build(&snap, &base);
    let tx = db.begin_read().map_err(SoError::Storage)?;

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

        let quoted_label = match cypher_ident(label) {
            Ok(q) => q,
            Err(e) => {
                report.unrepresentable_iris += 1;
                report.issues.push(ExportIssue {
                    subject: label.clone(),
                    reason: format!(
                        "label '{label}' cannot be safely used in a generated query ({e}); its \
                         nodes were not exported."
                    ),
                });
                continue;
            }
        };
        let q = format!("MATCH (n:{quoted_label}) RETURN id(n), n");
        let result = execute_or_empty(&tx, &q)?;
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
                let pred = match named(&pred_iri) {
                    Ok(p) => p,
                    Err(e) => {
                        report.unrepresentable_iris += 1;
                        report.issues.push(ExportIssue {
                            subject: subject_iri.clone(),
                            reason: format!(
                                "property '{}' on class '{label}' has an IRI that could not \
                                 be represented in RDF ({e}); this value was not exported.",
                                prop.name
                            ),
                        });
                        continue;
                    }
                };
                insert_triple(&mut out, Triple::new(subject.clone(), pred, obj));
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
        let pred = match named(rel_iri) {
            Ok(p) => p,
            Err(e) => {
                report.unrepresentable_iris += 1;
                report.issues.push(ExportIssue {
                    subject: rel.clone(),
                    reason: format!(
                        "relation '{rel}' has an IRI that could not be represented in RDF \
                         ({e}); its edges were not exported."
                    ),
                });
                continue;
            }
        };

        let quoted_rel = match cypher_ident(rel) {
            Ok(q) => q,
            Err(e) => {
                report.unrepresentable_iris += 1;
                report.issues.push(ExportIssue {
                    subject: rel.clone(),
                    reason: format!(
                        "relation type '{rel}' cannot be safely used in a generated query \
                         ({e}); its edges were not exported."
                    ),
                });
                continue;
            }
        };
        let q = format!("MATCH (a)-[r:{quoted_rel}]->(b) RETURN id(a), id(b)");
        let result = execute_or_empty(&tx, &q)?;
        for row in &result.rows {
            let (Some(ExecValue::Int64(src)), Some(ExecValue::Int64(dst))) =
                (row.first(), row.get(1))
            else {
                continue;
            };
            // Defensive: an edge endpoint may in principle name a node that no
            // longer exists, and emitting a triple for it would assert facts
            // about a deleted entity. Not currently reachable through the
            // public API on SparrowDB 0.1.27+ — `DELETE` on an edge-bearing
            // node is refused outright (`NodeHasEdges`, #436/PR #512) and
            // `DETACH DELETE` removes a node and its edges atomically, with
            // WAL replay applying each transaction's mutations all-or-nothing
            // (see the `dangling_edges` tests in test_rdf_data.rs). Kept for
            // a database created under an older SparrowDB, or a future
            // storage-layer regression.
            let (Some(s), Some(o)) = (subject_by_id.get(src), subject_by_id.get(dst)) else {
                report.dangling_edges += 1;
                report.issues.push(ExportIssue {
                    subject: format!("{src} -[{rel}]-> {dst}"),
                    reason: "Edge references a node that no longer exists; skipped.".to_string(),
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

    // Relations: object properties. Sorted for deterministic @context output —
    // idx.relation_iri is a HashMap, and if serde_json ever gains
    // preserve_order, HashMap iteration order would otherwise leak into the
    // serialized document and vary across runs.
    let mut relation_term_by_iri: HashMap<String, String> = HashMap::new();
    let mut relation_names: Vec<&String> = idx.relation_iri.keys().collect();
    relation_names.sort();
    for name in relation_names {
        let iri = &idx.relation_iri[name];
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
        if conflicting.contains(&p.name) || idx.relation_iri.contains_key(&p.name) {
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

/// The single conversion from a [`PropertyValue`] to the [`StoreValue`] bytes
/// SparrowDB stores. The MCP layer's `create_entity`/`update_entity` call this
/// directly rather than keeping their own copy — a second, drifted
/// implementation would mean entities written by import and entities written
/// by `create_entity` are encoded differently and read back with different
/// shapes. `Float64` is the fragile arm: it must go through [`fmt_f64`], not
/// a bare `to_string()`, so `f64::INFINITY`/`NEG_INFINITY` round-trip through
/// the XSD lexical forms (`INF`/`-INF`) that [`lexical_form`] expects on
/// export, instead of Rust's `Display` output (`inf`/`-inf`).
pub fn property_value_to_store(v: &PropertyValue) -> Option<StoreValue> {
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
/// # Not idempotent
/// Calling this twice against a database that already holds the data is
/// **not** a no-op: it can create duplicate entities and always duplicates
/// relationships. See the "Re-import is not idempotent" section in the module
/// docs before using this for incremental sync.
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
                if let Some(prev) = &entry.type_iri {
                    report.warnings.push(format!(
                        "<{subject_iri}> has more than one rdf:type; <{prev}> was dropped and \
                         <{}> was used. SparrowOntology v1 assigns one class per entity.",
                        n.as_str()
                    ));
                }
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
        if local.is_empty() {
            report.entities_skipped += 1;
            report.skips.push(ImportSkip {
                subject: iri.clone(),
                reason: format!(
                    "rdf:type <{type_iri}> has no local name. Use a type IRI that ends in a \
                     name, for example <{type_iri}Person>."
                ),
            });
            continue;
        }
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
    //
    // `idx` is only rebuilt after this loop (see `added` below), so a property
    // declared for one subject is invisible to `idx` for every later subject in
    // the same pass. Track what THIS pass has already declared in
    // `declared_this_pass` so a second subject of the same class carrying the
    // same property is recognised locally instead of re-declaring it — which
    // would otherwise hit `SoError::DuplicateProperty` and (via `?`) abort the
    // entire import over data that was already handled correctly.
    if strategy == ImportStrategy::AutoDeclare {
        let mut added = false;
        // Maps (class, resolved name) -> the first predicate IRI that
        // declared it this pass. A second, distinct predicate resolving to
        // the same name (two IRIs sharing a local name, both falling back to
        // `local_name`) would otherwise be silently dropped — declared_this_pass
        // being a plain set of (class, name) can't tell "already declared by
        // this exact predicate" from "declared by a colliding one", so it
        // swallows the second predicate with no report entry.
        let mut declared_this_pass: HashMap<(String, String), String> = HashMap::new();
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
                let key = (class.clone(), name.clone());
                if let Some(prior_pred) = declared_this_pass.get(&key) {
                    if prior_pred != pred {
                        report.warnings.push(format!(
                            "Subject <{iri}>: predicates <{prior_pred}> and <{pred}> both \
                             resolve to property name '{name}' on class '{class}'; only \
                             <{prior_pred}> was auto-declared as its source_iri. Declare \
                             <{pred}> explicitly with a distinct property name to keep both."
                        ));
                    }
                    continue;
                }
                declared_this_pass.insert(key, pred.clone());
                let ty = xsd_to_type_str(lit.datatype().as_str());
                match crate::init::add_property(
                    db,
                    class,
                    &name,
                    ty,
                    false,
                    false,
                    None,
                    None,
                    Some(pred),
                ) {
                    Ok(_) => {
                        report.properties_declared += 1;
                        added = true;
                    }
                    // Already declared by a concurrent/earlier caller outside
                    // this pass's own tracking — not an error for import.
                    Err(SoError::DuplicateProperty { .. }) => {}
                    Err(e) => return Err(e),
                }
            }
        }
        if added {
            idx = SchemaIndex::build(&export_schema(db)?, "");
        }
    }

    // ── Auto-declare missing relations ────────────────────────────────────────
    //
    // Same staleness issue as the property pass above: track relation names
    // this pass has already declared so a repeat only counts once in
    // `report.relations_declared` (write_relation_node itself is a safe
    // upsert via merge_node, so a repeat doesn't error — it just shouldn't be
    // double-counted).
    if strategy == ImportStrategy::AutoDeclare {
        let mut added = false;
        // Maps resolved relation name -> the first predicate IRI that
        // declared it this pass. Mirrors the property-declaration pass
        // above: relations, like properties, are keyed by name in the
        // ontology, so only the first predicate can be declared — but a
        // second, distinct predicate colliding on the same local name must
        // still be reported rather than dropped with no explanation (a
        // plain `HashSet` cannot tell "already declared by this exact
        // predicate" from "declared by a colliding one").
        let mut declared_this_pass: HashMap<String, String> = HashMap::new();
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
                if let Some(prior_pred) = declared_this_pass.get(&name) {
                    if prior_pred != pred {
                        report.warnings.push(format!(
                            "Subject <{iri}>: predicates <{prior_pred}> and <{pred}> both \
                             resolve to relation name '{name}'; only <{prior_pred}> was \
                             auto-declared as its source_iri. Declare <{pred}> explicitly with \
                             a distinct relation name to keep both."
                        ));
                    }
                    continue;
                }
                let Some(dst_class) = class_of.get(target) else {
                    continue; // handled as a skip in the relationship pass
                };
                declared_this_pass.insert(name.clone(), pred.clone());
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
        // Tracks which predicate IRI first populated each resolved property
        // name, so a second, distinct predicate that resolves to the same
        // name (e.g. two IRIs sharing a local name, both falling back to
        // `local_name`) is caught below instead of silently overwriting the
        // first value: the whole entity is skipped, same as any other
        // per-property failure in this loop (see `failed` below).
        let mut name_source: HashMap<String, String> = HashMap::new();
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
            // Two distinct predicates resolving to the same property name
            // (they share a local name and neither has a distinct
            // source_iri-registered property) is ambiguous, not resolvable
            // by picking either value — skip the entity rather than
            // silently keep one and discard the other.
            if let Some(prior_pred) = name_source.get(&name) {
                if prior_pred != pred {
                    failed = Some(format!(
                        "Subject <{iri}>: predicates <{prior_pred}> and <{pred}> both resolve to \
                         property '{name}' on class '{class}' (they share a local name and \
                         neither has a distinct source_iri-registered property). This entity was \
                         not imported — give one of them an explicit, source_iri-registered \
                         property to disambiguate."
                    ));
                    break;
                }
            }
            match literal_to_property_value(&prop.datatype, lit) {
                Ok(v) => {
                    name_source.insert(name.clone(), pred.clone());
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
        // Tracks which predicate IRI first resolved to each relation name for
        // THIS subject. Unlike the entity-property case above, a relation can
        // legitimately be multi-valued (the same predicate pointing at
        // several targets is normal and not a collision), so a match here
        // does not block edge creation — both edges are still written. But
        // two *distinct* predicates silently merging into one relation type
        // (they share a local name and neither has a distinct
        // source_iri-registered relation) is a real ambiguity that would
        // otherwise vanish with no trace, so it is reported.
        let mut rel_name_source: HashMap<String, String> = HashMap::new();
        for (pred, target) in &data.links {
            let rel_name = idx
                .relation_by_iri
                .get(pred)
                .cloned()
                .unwrap_or_else(|| local_name(pred));

            match rel_name_source.get(&rel_name) {
                Some(prior_pred) if prior_pred != pred => {
                    report.warnings.push(format!(
                        "Subject <{iri}>: predicates <{prior_pred}> and <{pred}> both resolve to \
                         relation '{rel_name}' (they share a local name and neither has a \
                         distinct source_iri-registered relation). Both edges were created under \
                         '{rel_name}' — give one of them an explicit, source_iri-registered \
                         relation to disambiguate."
                    ));
                }
                Some(_) => {}
                None => {
                    rel_name_source.insert(rel_name.clone(), pred.clone());
                }
            }

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

    /// Two distinct predicate IRIs that share a local name ("desc") and are
    /// both unregistered (no source_iri on either) resolve to the *same*
    /// ontology property on import. Neither value is more correct than the
    /// other, so the entity must be skipped and reported — never partially
    /// imported with one value silently discarded.
    ///
    /// Expected values derived by hand from the fixture: one subject, two
    /// colliding predicates, zero unambiguous property assignments possible.
    #[test]
    fn colliding_predicate_local_names_skip_the_entity() {
        let dir = tempfile::tempdir().unwrap();
        let db = GraphDb::open(dir.path()).unwrap();

        write_class_node(&db, "Person", "", "https://ex.test/schema/Person").unwrap();
        crate::init::add_property(
            &db, "Person", "desc", "string", false, false, None, None, None,
        )
        .unwrap();

        let turtle = r#"
            @prefix a: <http://ns-a.example/> .
            @prefix b: <http://ns-b.example/> .
            <http://ex.test/p1> a <https://ex.test/schema/Person> ;
                a:desc "internal notes" ;
                b:desc "public bio" .
        "#;

        let report = import_data_turtle(&db, turtle, ImportStrategy::Strict).unwrap();

        assert_eq!(
            report.entities_imported, 0,
            "the colliding entity must not be counted as imported"
        );
        assert_eq!(report.entities_skipped, 1, "it must be counted as skipped");
        assert_eq!(report.skips.len(), 1);
        assert!(
            report.skips[0].reason.contains("desc"),
            "skip reason should name the colliding property: {}",
            report.skips[0].reason
        );

        // No node of class Person should have been written at all.
        let count = db
            .execute("MATCH (n:Person) RETURN count(n)")
            .unwrap()
            .rows
            .first()
            .and_then(|r| r.first().cloned());
        assert_eq!(
            count,
            Some(ExecValue::Int64(0)),
            "no Person node should exist — the collision must block the write, \
             not just warn after one value already overwrote the other"
        );
    }

    /// Two distinct predicate IRIs sharing a local name ("knows") that
    /// *neither* is registered for, imported with `AutoDeclare`. Exercises
    /// both remaining `local_name(pred)` fallback sites: relation
    /// auto-declaration and relationship creation.
    ///
    /// Unlike the entity-property case, a relation can legitimately be
    /// multi-valued (one subject, several targets, same predicate is normal),
    /// so this must NOT block edge creation — both edges are expected to be
    /// written. What must NOT happen is the two distinct predicates merging
    /// into one relation type with no trace of the ambiguity.
    ///
    /// Expected values derived by hand from the fixture:
    /// - 3 entities (s1, s2, s3), all class Person.
    /// - `a:knows` is encountered first in Turtle order for s1's links, so it
    ///   is the one auto-declared as relation "knows" (relations_declared=1,
    ///   not 2 — the ontology cannot hold two relations named "knows").
    /// - `b:knows` collides with the already-declared "knows" in BOTH the
    ///   auto-declare pass (warn, don't re-declare) and the
    ///   relationship-creation pass (warn, but still create the edge) — 2
    ///   warnings total.
    /// - Both s1->s2 (via a:knows) and s1->s3 (via b:knows) edges are
    ///   created: relationships_imported=2, relationships_skipped=0.
    #[test]
    fn colliding_relation_local_names_are_warned_not_silently_merged() {
        let dir = tempfile::tempdir().unwrap();
        let db = GraphDb::open(dir.path()).unwrap();

        write_class_node(&db, "Person", "", "https://ex.test/schema/Person").unwrap();

        let turtle = r#"
            @prefix a: <http://ns-a.example/> .
            @prefix b: <http://ns-b.example/> .
            <http://ex.test/s1> a <https://ex.test/schema/Person> .
            <http://ex.test/s2> a <https://ex.test/schema/Person> .
            <http://ex.test/s3> a <https://ex.test/schema/Person> .
            <http://ex.test/s1> a:knows <http://ex.test/s2> .
            <http://ex.test/s1> b:knows <http://ex.test/s3> .
        "#;

        let report = import_data_turtle(&db, turtle, ImportStrategy::AutoDeclare).unwrap();

        assert_eq!(report.entities_imported, 3, "s1, s2, s3 all import cleanly");
        assert_eq!(report.entities_skipped, 0);
        assert_eq!(
            report.relations_declared, 1,
            "only the first-seen predicate (a:knows) is auto-declared as 'knows'"
        );
        assert_eq!(
            report.relationships_imported, 2,
            "both edges must still be created — a relation can be multi-valued, \
             the collision is about naming, not about which edge is 'correct'"
        );
        assert_eq!(report.relationships_skipped, 0);

        let collision_warnings: Vec<&String> = report
            .warnings
            .iter()
            .filter(|w| w.contains("knows"))
            .collect();
        assert_eq!(
            collision_warnings.len(),
            2,
            "expected one warning from the auto-declare pass and one from the \
             relationship-creation pass, got: {:?}",
            report.warnings
        );
    }
}
