//! Schema snapshot: export and import the full ontology schema.
//!
//! Use `export_schema` to capture all `__SO_*` nodes and edges into a
//! serialisable `SchemaSnapshot`.  Use `import_schema` to replay that
//! snapshot into a fresh (or blank-init'd) database.
//!
//! Intended upgrade path when the underlying SparrowDB WAL format changes:
//!
//! ```text
//! let snap = export_schema(&old_db)?;
//! let json  = serde_json::to_string(&snap)?;
//! // … open fresh DB at new SparrowDB version …
//! let snap2: SchemaSnapshot = serde_json::from_str(&json)?;
//! import_schema(&new_db, &snap2)?;
//! ```

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use sparrowdb::GraphDb;
use sparrowdb_common::NodeId;
use sparrowdb_execution::Value;
use sparrowdb_storage::node_store::Value as StoreValue;

use crate::error::SoError;
use crate::model::{
    AliasKind, OntologyAlias, OntologyClass, OntologyProperty, OntologyRelation, OwnerKind,
    PropertyType, SymbolStatus,
};
use crate::namespace::{
    ALIAS_LABEL, ALIAS_OF_REL, CLASS_LABEL, DOMAIN_REL, HAS_PROPERTY_REL, PROPERTY_LABEL,
    RANGE_REL, RELATION_LABEL, SUBCLASS_OF_REL,
};
use crate::resolution::escape_cypher_string;

// ── Public snapshot types ────────────────────────────────────────────────────

pub const SNAPSHOT_VERSION: u32 = 1;

/// A complete, portable snapshot of all ontology schema nodes and edges.
///
/// Serialises to / from JSON via serde. The `snapshot_version` field allows
/// future breaking changes to the snapshot format to be detected at import time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaSnapshot {
    pub snapshot_version: u32,
    pub exported_at: i64,
    pub classes: Vec<OntologyClass>,
    pub relations: Vec<OntologyRelation>,
    pub properties: Vec<OntologyProperty>,
    pub aliases: Vec<OntologyAlias>,
    /// `(child_symbol_id, parent_symbol_id)` pairs for `__SO_SUBCLASS_OF` edges.
    pub subclass_edges: Vec<(String, String)>,
}

#[derive(Debug)]
pub struct ImportSchemaResult {
    pub classes_imported: usize,
    pub relations_imported: usize,
    pub properties_imported: usize,
    pub aliases_imported: usize,
    pub subclass_edges_imported: usize,
}

// ── Low-level value helpers (mirrors init.rs, local to this module) ──────────

fn sv(s: &str) -> StoreValue {
    StoreValue::Bytes(s.as_bytes().to_vec())
}

fn bv(b: bool) -> StoreValue {
    StoreValue::Int64(if b { 1 } else { 0 })
}

fn iv(n: i64) -> StoreValue {
    StoreValue::Int64(n)
}

fn kv(pairs: &[(&str, StoreValue)]) -> HashMap<String, StoreValue> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before Unix epoch")
        .as_millis() as i64
}

// ── Exec-value coercion helpers ───────────────────────────────────────────────

/// Extract a String from a query result Value, or empty string.
fn str_val(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        _ => String::new(),
    }
}

/// Extract a non-empty String, or None.
fn opt_str_val(v: &Value) -> Option<String> {
    match v {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        _ => None,
    }
}

/// Extract a bool stored as either Value::Bool or Value::Int64(1/0).
fn bool_val(v: &Value) -> bool {
    match v {
        Value::Bool(b) => *b,
        Value::Int64(n) => *n != 0,
        _ => false,
    }
}

/// Extract an i64, or 0.
fn i64_val(v: &Value) -> i64 {
    match v {
        Value::Int64(n) => *n,
        _ => 0,
    }
}

fn status_from_str(s: &str) -> SymbolStatus {
    if s == "deprecated" {
        SymbolStatus::Deprecated
    } else {
        SymbolStatus::Active
    }
}

fn status_to_str(s: &SymbolStatus) -> &'static str {
    match s {
        SymbolStatus::Deprecated => "deprecated",
        SymbolStatus::Active => "active",
    }
}

fn property_type_from_str(s: &str) -> PropertyType {
    match s {
        "int64" => PropertyType::Int64,
        "float64" => PropertyType::Float64,
        "bool" => PropertyType::Bool,
        "date" => PropertyType::Date,
        "variant" => PropertyType::Variant,
        _ => PropertyType::String,
    }
}

fn property_type_to_str(t: &PropertyType) -> &'static str {
    match t {
        PropertyType::String => "string",
        PropertyType::Int64 => "int64",
        PropertyType::Float64 => "float64",
        PropertyType::Bool => "bool",
        PropertyType::Date => "date",
        PropertyType::Variant => "variant",
    }
}

// ── Export ───────────────────────────────────────────────────────────────────

/// Export all `__SO_*` ontology schema nodes and edges to a portable snapshot.
pub fn export_schema(db: &GraphDb) -> Result<SchemaSnapshot, SoError> {
    let classes = export_classes(db)?;
    let relations = export_relations(db)?;

    // symbol_id → name map for owner_name reconstruction in properties
    let class_name_by_sid: HashMap<String, String> = classes
        .iter()
        .map(|c| (c.symbol_id.clone(), c.name.clone()))
        .collect();

    let properties = export_properties(db, &class_name_by_sid)?;
    let aliases = export_aliases(db)?;
    let subclass_edges = export_subclass_edges(db)?;

    Ok(SchemaSnapshot {
        snapshot_version: SNAPSHOT_VERSION,
        exported_at: now_ms(),
        classes,
        relations,
        properties,
        aliases,
        subclass_edges,
    })
}

fn export_classes(db: &GraphDb) -> Result<Vec<OntologyClass>, SoError> {
    let q = format!(
        "MATCH (n:{CLASS_LABEL}) \
         RETURN n.symbol_id, n.name, n.description, n.status, n.created_at, n.updated_at"
    );
    let result = match db.execute(&q) {
        Ok(r) => r,
        Err(sparrowdb_common::Error::InvalidArgument(ref msg))
            if msg.contains("unknown label") =>
        {
            return Ok(vec![]);
        }
        Err(e) => return Err(SoError::Storage(e)),
    };
    let mut out = Vec::new();
    for row in &result.rows {
        if row.len() < 6 {
            continue;
        }
        out.push(OntologyClass {
            symbol_id: str_val(&row[0]),
            name: str_val(&row[1]),
            description: opt_str_val(&row[2]),
            status: status_from_str(&str_val(&row[3])),
            created_at: i64_val(&row[4]),
            updated_at: i64_val(&row[5]),
        });
    }
    Ok(out)
}

fn export_relations(db: &GraphDb) -> Result<Vec<OntologyRelation>, SoError> {
    let q = format!(
        "MATCH (r:{RELATION_LABEL}) \
         RETURN r.symbol_id, r.name, r.description, r.status, r.directed, r.created_at, r.updated_at"
    );
    let result = match db.execute(&q) {
        Ok(r) => r,
        Err(sparrowdb_common::Error::InvalidArgument(ref msg))
            if msg.contains("unknown label") =>
        {
            return Ok(vec![]);
        }
        Err(e) => return Err(SoError::Storage(e)),
    };

    // Collect node-level data first
    struct RelBase {
        symbol_id: String,
        name: String,
        description: Option<String>,
        status: SymbolStatus,
        directed: bool,
        created_at: i64,
        updated_at: i64,
    }
    let mut bases: Vec<RelBase> = Vec::new();
    for row in &result.rows {
        if row.len() < 7 {
            continue;
        }
        bases.push(RelBase {
            symbol_id: str_val(&row[0]),
            name: str_val(&row[1]),
            description: opt_str_val(&row[2]),
            status: status_from_str(&str_val(&row[3])),
            directed: bool_val(&row[4]),
            created_at: i64_val(&row[5]),
            updated_at: i64_val(&row[6]),
        });
    }

    let domain_map = query_rel_class_edges(db, DOMAIN_REL)?;
    let range_map = query_rel_class_edges(db, RANGE_REL)?;

    let out = bases
        .into_iter()
        .map(|b| OntologyRelation {
            domain: domain_map.get(&b.symbol_id).cloned().unwrap_or_default(),
            range: range_map.get(&b.symbol_id).cloned().unwrap_or_default(),
            symbol_id: b.symbol_id,
            name: b.name,
            description: b.description,
            status: b.status,
            directed: b.directed,
            created_at: b.created_at,
            updated_at: b.updated_at,
        })
        .collect();
    Ok(out)
}

/// Returns a map of relation_symbol_id → class_name for a given edge type.
fn query_rel_class_edges(
    db: &GraphDb,
    edge_type: &str,
) -> Result<HashMap<String, String>, SoError> {
    let q = format!(
        "MATCH (r:{RELATION_LABEL})-[:{edge_type}]->(c:{CLASS_LABEL}) \
         RETURN r.symbol_id, c.name"
    );
    let result = match db.execute(&q) {
        Ok(r) => r,
        Err(sparrowdb_common::Error::InvalidArgument(ref msg))
            if msg.contains("unknown label") || msg.contains("unknown relationship type") =>
        {
            return Ok(HashMap::new());
        }
        Err(e) => return Err(SoError::Storage(e)),
    };
    let mut map = HashMap::new();
    for row in &result.rows {
        if row.len() < 2 {
            continue;
        }
        let rel_sid = str_val(&row[0]);
        let class_name = str_val(&row[1]);
        if !rel_sid.is_empty() && !class_name.is_empty() {
            map.insert(rel_sid, class_name);
        }
    }
    Ok(map)
}

fn export_properties(
    db: &GraphDb,
    class_name_by_sid: &HashMap<String, String>,
) -> Result<Vec<OntologyProperty>, SoError> {
    let q = format!(
        "MATCH (p:{PROPERTY_LABEL}) \
         RETURN p.symbol_id, p.name, p.datatype, p.required, p.unique, \
                p.enum_values, p.owner_symbol_id, p.owner_kind, p.created_at"
    );
    let result = match db.execute(&q) {
        Ok(r) => r,
        Err(sparrowdb_common::Error::InvalidArgument(ref msg))
            if msg.contains("unknown label") =>
        {
            return Ok(vec![]);
        }
        Err(e) => return Err(SoError::Storage(e)),
    };
    let mut out = Vec::new();
    for row in &result.rows {
        if row.len() < 9 {
            continue;
        }
        let symbol_id = str_val(&row[0]);
        let name = str_val(&row[1]);
        let datatype = property_type_from_str(&str_val(&row[2]));
        let required = bool_val(&row[3]);
        let unique = bool_val(&row[4]);
        let enum_json = str_val(&row[5]);
        let allowed_values: Option<Vec<String>> = if enum_json.is_empty() {
            None
        } else {
            serde_json::from_str(&enum_json)
                .ok()
                .filter(|v: &Vec<String>| !v.is_empty())
        };
        let owner_symbol_id = str_val(&row[6]);
        let owner_kind_str = str_val(&row[7]);
        let created_at = i64_val(&row[8]);
        let owner_name = class_name_by_sid
            .get(&owner_symbol_id)
            .cloned()
            .unwrap_or_default();
        let owner_kind = if owner_kind_str == "relation" {
            OwnerKind::Relation
        } else {
            OwnerKind::Class
        };
        out.push(OntologyProperty {
            symbol_id,
            name,
            datatype,
            required,
            unique,
            allowed_values,
            default_value: None,
            owner_symbol_id,
            owner_kind,
            created_at,
            owner_name,
        });
    }
    Ok(out)
}

fn export_aliases(db: &GraphDb) -> Result<Vec<OntologyAlias>, SoError> {
    // Follow the ALIAS_OF edge to get the target's symbol_id.
    let q = format!(
        "MATCH (a:{ALIAS_LABEL})-[:{ALIAS_OF_REL}]->(t) \
         RETURN a.name, a.kind, a.target_name, t.symbol_id, a.created_at"
    );
    let result = match db.execute(&q) {
        Ok(r) => r,
        Err(sparrowdb_common::Error::InvalidArgument(ref msg))
            if msg.contains("unknown label") || msg.contains("unknown relationship type") =>
        {
            return Ok(vec![]);
        }
        Err(e) => return Err(SoError::Storage(e)),
    };
    let mut out = Vec::new();
    for row in &result.rows {
        if row.len() < 5 {
            continue;
        }
        let name = str_val(&row[0]);
        let kind_str = str_val(&row[1]);
        let target_name = str_val(&row[2]);
        let target_symbol_id = str_val(&row[3]);
        let created_at = i64_val(&row[4]);
        let kind = if kind_str == "relation" {
            AliasKind::Relation
        } else {
            AliasKind::Class
        };
        out.push(OntologyAlias {
            name,
            kind,
            target_symbol_id,
            target_name,
            created_at,
        });
    }
    Ok(out)
}

fn export_subclass_edges(db: &GraphDb) -> Result<Vec<(String, String)>, SoError> {
    let q = format!(
        "MATCH (child:{CLASS_LABEL})-[:{SUBCLASS_OF_REL}]->(parent:{CLASS_LABEL}) \
         RETURN child.symbol_id, parent.symbol_id"
    );
    let result = match db.execute(&q) {
        Ok(r) => r,
        Err(sparrowdb_common::Error::InvalidArgument(ref msg))
            if msg.contains("unknown label") || msg.contains("unknown relationship type") =>
        {
            return Ok(vec![]);
        }
        Err(e) => return Err(SoError::Storage(e)),
    };
    let mut out = Vec::new();
    for row in &result.rows {
        if row.len() < 2 {
            continue;
        }
        let child_sid = str_val(&row[0]);
        let parent_sid = str_val(&row[1]);
        if !child_sid.is_empty() && !parent_sid.is_empty() {
            out.push((child_sid, parent_sid));
        }
    }
    Ok(out)
}

// ── Import ───────────────────────────────────────────────────────────────────

/// Import a `SchemaSnapshot` into `db`.
///
/// The database should be fresh (no prior `init()` call needed — `import_schema`
/// creates labels and indices itself).  Importing into a DB that already has
/// matching nodes is safe: `merge_node` is idempotent on `symbol_id`.
///
/// Does NOT touch user graph data (non-`__SO_*` nodes/edges).
pub fn import_schema(
    db: &GraphDb,
    snapshot: &SchemaSnapshot,
) -> Result<ImportSchemaResult, SoError> {
    // ── 1. Class nodes ────────────────────────────────────────────────────────
    let mut class_node_ids: HashMap<String, NodeId> = HashMap::new();
    for c in &snapshot.classes {
        let nid = import_class_node(db, c)?;
        class_node_ids.insert(c.symbol_id.clone(), nid);
    }

    // ── 2. Relation nodes ─────────────────────────────────────────────────────
    let mut relation_node_ids: HashMap<String, NodeId> = HashMap::new();
    for r in &snapshot.relations {
        let nid = import_relation_node(db, r)?;
        relation_node_ids.insert(r.symbol_id.clone(), nid);
    }

    // ── 3. DOMAIN + RANGE edges ───────────────────────────────────────────────
    let class_nid_by_name: HashMap<String, NodeId> = snapshot
        .classes
        .iter()
        .filter_map(|c| class_node_ids.get(&c.symbol_id).map(|&id| (c.name.clone(), id)))
        .collect();

    for r in &snapshot.relations {
        let rel_nid = *relation_node_ids
            .get(&r.symbol_id)
            .ok_or(SoError::Storage(sparrowdb_common::Error::NotFound))?;
        if let Some(&domain_nid) = class_nid_by_name.get(&r.domain) {
            let mut tx = db.begin_write()?;
            tx.create_edge(rel_nid, domain_nid, DOMAIN_REL, HashMap::new())?;
            tx.commit()?;
        }
        if let Some(&range_nid) = class_nid_by_name.get(&r.range) {
            let mut tx = db.begin_write()?;
            tx.create_edge(rel_nid, range_nid, RANGE_REL, HashMap::new())?;
            tx.commit()?;
        }
    }

    // ── 4. Property nodes + HAS_PROPERTY edges + UNIQUE constraints ───────────
    let class_info_by_sid: HashMap<String, (NodeId, String)> = snapshot
        .classes
        .iter()
        .filter_map(|c| {
            class_node_ids
                .get(&c.symbol_id)
                .map(|&id| (c.symbol_id.clone(), (id, c.name.clone())))
        })
        .collect();

    for p in &snapshot.properties {
        let prop_nid = import_property_node(db, p)?;
        if let Some(&(owner_nid, _)) = class_info_by_sid.get(&p.owner_symbol_id) {
            let mut tx = db.begin_write()?;
            tx.create_edge(owner_nid, prop_nid, HAS_PROPERTY_REL, HashMap::new())?;
            tx.commit()?;
        }
        // Re-emit UNIQUE constraint
        if p.unique {
            if let Some((_, ref class_name)) = class_info_by_sid.get(&p.owner_symbol_id) {
                let safe_class = escape_cypher_string(class_name);
                let safe_prop = escape_cypher_string(&p.name);
                let cq = format!(
                    "CREATE CONSTRAINT ON (n:{safe_class}) ASSERT n.{safe_prop} IS UNIQUE"
                );
                db.execute(&cq).map_err(SoError::Storage)?;
            }
        }
    }

    // ── 5. Alias nodes + ALIAS_OF edges ──────────────────────────────────────
    let mut aliases_imported = 0;
    for a in &snapshot.aliases {
        crate::init::add_alias(db, &a.name, a.kind.clone(), &a.target_name)?;
        aliases_imported += 1;
    }

    // ── 6. SUBCLASS_OF edges ──────────────────────────────────────────────────
    let mut subclass_edges_imported = 0;
    for (child_sid, parent_sid) in &snapshot.subclass_edges {
        if let (Some(&child_nid), Some(&parent_nid)) = (
            class_node_ids.get(child_sid),
            class_node_ids.get(parent_sid),
        ) {
            let mut tx = db.begin_write()?;
            tx.create_edge(child_nid, parent_nid, SUBCLASS_OF_REL, HashMap::new())?;
            tx.commit()?;
            subclass_edges_imported += 1;
        }
    }

    // ── 7. Schema indices ─────────────────────────────────────────────────────
    for label in &[CLASS_LABEL, RELATION_LABEL, PROPERTY_LABEL] {
        let q = format!("CREATE INDEX ON :{label}(name)");
        match db.execute(&q) {
            Ok(_) => {}
            Err(sparrowdb_common::Error::InvalidArgument(ref msg))
                if msg.contains("unknown label") || msg.contains("already exists") => {}
            Err(e) => return Err(SoError::Storage(e)),
        }
    }

    Ok(ImportSchemaResult {
        classes_imported: class_node_ids.len(),
        relations_imported: relation_node_ids.len(),
        properties_imported: snapshot.properties.len(),
        aliases_imported,
        subclass_edges_imported,
    })
}

// ── Private import node helpers ───────────────────────────────────────────────

fn import_class_node(db: &GraphDb, c: &OntologyClass) -> Result<NodeId, SoError> {
    let desc = c.description.as_deref().unwrap_or("");
    let mut tx = db.begin_write()?;
    let nid = tx.merge_node(
        CLASS_LABEL,
        kv(&[
            ("symbol_id", sv(&c.symbol_id)),
            ("name", sv(&c.name)),
            ("description", sv(desc)),
            ("status", sv(status_to_str(&c.status))),
            ("created_at", iv(c.created_at)),
            ("updated_at", iv(c.updated_at)),
        ]),
    )?;
    tx.commit()?;
    Ok(nid)
}

fn import_relation_node(db: &GraphDb, r: &OntologyRelation) -> Result<NodeId, SoError> {
    let desc = r.description.as_deref().unwrap_or("");
    let mut tx = db.begin_write()?;
    let nid = tx.merge_node(
        RELATION_LABEL,
        kv(&[
            ("symbol_id", sv(&r.symbol_id)),
            ("name", sv(&r.name)),
            ("description", sv(desc)),
            ("status", sv(status_to_str(&r.status))),
            ("directed", bv(r.directed)),
            ("created_at", iv(r.created_at)),
            ("updated_at", iv(r.updated_at)),
        ]),
    )?;
    tx.commit()?;
    Ok(nid)
}

fn import_property_node(db: &GraphDb, p: &OntologyProperty) -> Result<NodeId, SoError> {
    let datatype_str = property_type_to_str(&p.datatype);
    let enum_json = p
        .allowed_values
        .as_deref()
        .map(|v| serde_json::to_string(v).unwrap_or_default())
        .unwrap_or_default();
    let owner_kind_str = match p.owner_kind {
        OwnerKind::Relation => "relation",
        OwnerKind::Class => "class",
    };
    let mut tx = db.begin_write()?;
    let nid = tx.merge_node(
        PROPERTY_LABEL,
        kv(&[
            ("symbol_id", sv(&p.symbol_id)),
            ("name", sv(&p.name)),
            ("datatype", sv(datatype_str)),
            ("required", bv(p.required)),
            ("unique", bv(p.unique)),
            ("enum_values", sv(&enum_json)),
            ("owner_symbol_id", sv(&p.owner_symbol_id)),
            ("owner_kind", sv(owner_kind_str)),
            ("created_at", iv(p.created_at)),
        ]),
    )?;
    tx.commit()?;
    Ok(nid)
}
