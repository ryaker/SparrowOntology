use std::collections::HashMap;

use sparrowdb::GraphDb;
use sparrowdb_common::NodeId;
use sparrowdb_execution::Value as ExecValue;
use sparrowdb_storage::node_store::Value as StoreValue;

use crate::error::SoError;
use crate::model::{
    canonical_world_model, canonical_world_model_properties, canonical_world_model_relations,
    personal_knowledge_classes, personal_knowledge_properties, personal_knowledge_relations,
    professional_network_classes, professional_network_properties, professional_network_relations,
    research_notes_classes, research_notes_properties, research_notes_relations,
    AliasKind, OntologyClass, OntologyProperty, OntologyRelation, PropertyType,
};
use crate::namespace::{
    ALIAS_LABEL, ALIAS_OF_REL, CLASS_LABEL, DOMAIN_REL, HAS_PROPERTY_REL, PROPERTY_LABEL,
    RANGE_REL, RELATION_LABEL, SUBCLASS_OF_REL,
};
use crate::resolution::{escape_cypher_string, resolve};
use crate::hierarchy::check_no_cycle;

// ── Public types ──────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct InitResult {
    pub classes_created: usize,
    pub relations_created: usize,
    pub properties_created: usize,
    pub starter: StarterKind,
}

#[derive(Debug)]
pub enum StarterKind {
    WorldModel,
    Blank,
    PersonalKnowledge,
    ProfessionalNetwork,
    ResearchNotes,
}

// ── init ──────────────────────────────────────────────────────────────────────

/// Initialize the ontology in `db`.
///
/// - `starter`: which starter kit to use (`None` defaults to `WorldModel`).
/// - `force`: if `true`, skips the already-initialized check and re-seeds.
///
/// Returns `SoError::AlreadyInitialized` if the DB already has `__SO_Class`
/// nodes and `force` is `false`.
///
/// NOTE: Uses `WriteTx` low-level API (create_label, merge_node, create_edge)
/// to bypass the Cypher-layer `__SO_` CREATE reservation. WriteTx bypasses
/// Cypher checks by design — no privileged write API is needed.
///
/// NOTE: `force=true` currently re-seeds via idempotent MERGE (existing nodes
/// are overwritten). A full wipe via `MATCH (n:__SO_Class) DELETE n` is
/// unblocked (SparrowDB 0.1.2 MATCH…DELETE does not check reserved labels)
/// but requires edge deletion first — tracked as a follow-up.
pub fn init(
    db: &GraphDb,
    starter: Option<StarterKind>,
    force: bool,
) -> Result<InitResult, SoError> {
    // Check if already initialized.
    // SparrowDB returns InvalidArgument("unknown label") if the label hasn't been created.
    let count = {
        let q = format!("MATCH (n:{CLASS_LABEL}) RETURN count(n)");
        match db.execute(&q) {
            Ok(result) => result
                .rows
                .first()
                .and_then(|r| r.first())
                .map(|v| match v {
                    ExecValue::Int64(n) => *n,
                    _ => 0,
                })
                .unwrap_or(0),
            Err(sparrowdb_common::Error::InvalidArgument(ref msg))
                if msg.contains("unknown label") =>
            {
                0
            }
            Err(e) => return Err(SoError::Storage(e)),
        }
    };

    if count > 0 && !force {
        return Err(SoError::AlreadyInitialized);
    }

    // force=true: currently re-seeds (idempotent). Full wipe (delete all __SO_* nodes
    // and edges) is unblocked — MATCH…DELETE works in SparrowDB 0.1.2.
    // Re-seeding is idempotent: create_label is idempotent, existing nodes
    // with same symbol_id are simply re-written over the same storage slot.

    let resolved_starter = starter.unwrap_or(StarterKind::WorldModel);

    let (classes, relations, properties) = match &resolved_starter {
        StarterKind::WorldModel => (
            canonical_world_model(),
            canonical_world_model_relations(),
            canonical_world_model_properties(),
        ),
        StarterKind::Blank => (vec![], vec![], vec![]),
        StarterKind::PersonalKnowledge => (
            personal_knowledge_classes(),
            personal_knowledge_relations(),
            personal_knowledge_properties(),
        ),
        StarterKind::ProfessionalNetwork => (
            professional_network_classes(),
            professional_network_relations(),
            professional_network_properties(),
        ),
        StarterKind::ResearchNotes => (
            research_notes_classes(),
            research_notes_relations(),
            research_notes_properties(),
        ),
    };

    // Seed classes first — track name → NodeId for edge creation
    let mut class_ids: HashMap<String, NodeId> = HashMap::new();
    for c in &classes {
        let node_id = seed_class(db, c)?;
        class_ids.insert(c.name.clone(), node_id);
    }

    // Seed relations — track name → NodeId for edge creation
    let mut relation_ids: HashMap<String, NodeId> = HashMap::new();
    for r in &relations {
        let node_id = seed_relation_node(db, r)?;
        relation_ids.insert(r.name.clone(), node_id);
    }

    // Create DOMAIN and RANGE edges for each relation
    for r in &relations {
        let rel_id = relation_ids[&r.name];
        let domain_id = *class_ids.get(&r.domain).ok_or_else(|| SoError::UnknownSymbol {
            name: r.domain.clone(),
            kind: "class".to_string(),
            valid: class_ids.keys().cloned().collect(),
            closest_match: None,
            suggestion: None,
        })?;
        let range_id = *class_ids.get(&r.range).ok_or_else(|| SoError::UnknownSymbol {
            name: r.range.clone(),
            kind: "class".to_string(),
            valid: class_ids.keys().cloned().collect(),
            closest_match: None,
            suggestion: None,
        })?;

        let mut tx = db.begin_write()?;
        tx.create_edge(rel_id, domain_id, DOMAIN_REL, HashMap::new())?;
        tx.create_edge(rel_id, range_id, RANGE_REL, HashMap::new())?;
        tx.commit()?;
    }

    // Seed properties — track symbol_id → NodeId
    let mut property_ids: HashMap<String, NodeId> = HashMap::new();
    for p in &properties {
        let node_id = seed_property_node(db, p, &classes)?;
        property_ids.insert(p.symbol_id.clone(), node_id);
    }

    // Create HAS_PROPERTY edges from class → property
    for p in &properties {
        let owner_class = classes.iter().find(|c| c.name == p.owner_name).ok_or_else(|| {
            SoError::UnknownSymbol {
                name: p.owner_name.clone(),
                kind: "class".to_string(),
                valid: classes.iter().map(|c| c.name.clone()).collect(),
                closest_match: None,
                suggestion: None,
            }
        })?;
        let class_id = class_ids[&owner_class.name];
        let prop_id = property_ids[&p.symbol_id];

        let mut tx = db.begin_write()?;
        tx.create_edge(class_id, prop_id, HAS_PROPERTY_REL, HashMap::new())?;
        tx.commit()?;
    }

    // Emit schema indices on internal labels for faster ontology lookups.
    // Wrapped in individual match arms so a missing label (empty starter) doesn't abort.
    for label in &[CLASS_LABEL, RELATION_LABEL, PROPERTY_LABEL] {
        let q = format!("CREATE INDEX ON :{label}(name)");
        match db.execute(&q) {
            Ok(_) => {}
            Err(sparrowdb_common::Error::InvalidArgument(ref msg))
                if msg.contains("unknown label") || msg.contains("already exists") =>
            {
                // no nodes seeded yet (Blank) or index already present — ignore
            }
            Err(e) => return Err(SoError::Storage(e)),
        }
    }

    Ok(InitResult {
        classes_created: classes.len(),
        relations_created: relations.len(),
        properties_created: properties.len(),
        starter: resolved_starter,
    })
}

// ── Low-level seeding via WriteTx (bypasses Cypher __SO_ reservation) ─────────

fn sv(s: &str) -> StoreValue {
    StoreValue::Bytes(s.as_bytes().to_vec())
}

fn bv(b: bool) -> StoreValue {
    StoreValue::Int64(if b { 1 } else { 0 })
}

fn iv(n: i64) -> StoreValue {
    StoreValue::Int64(n)
}

fn props(pairs: &[(&str, StoreValue)]) -> HashMap<String, StoreValue> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect()
}

/// Seed one class node. Returns the NodeId.
///
/// Uses `merge_node` which does get-or-create for the label (idempotent).
/// Matches on ALL provided properties; unique per symbol_id.
fn seed_class(db: &GraphDb, c: &OntologyClass) -> Result<NodeId, SoError> {
    let desc = c.description.as_deref().unwrap_or("");
    let mut tx = db.begin_write()?;
    let node_id = tx.merge_node(
        CLASS_LABEL,
        props(&[
            ("symbol_id", sv(&c.symbol_id)),
            ("name", sv(&c.name)),
            ("description", sv(desc)),
            ("status", sv("active")),
            ("created_at", iv(c.created_at)),
            ("updated_at", iv(c.updated_at)),
        ]),
    )?;
    tx.commit()?;
    Ok(node_id)
}

/// Seed one relation node (without edges). Returns the NodeId.
fn seed_relation_node(db: &GraphDb, r: &OntologyRelation) -> Result<NodeId, SoError> {
    let desc = r.description.as_deref().unwrap_or("");
    let mut tx = db.begin_write()?;
    let node_id = tx.merge_node(
        RELATION_LABEL,
        props(&[
            ("symbol_id", sv(&r.symbol_id)),
            ("name", sv(&r.name)),
            ("description", sv(desc)),
            ("status", sv("active")),
            ("directed", bv(r.directed)),
            ("created_at", iv(r.created_at)),
            ("updated_at", iv(r.updated_at)),
        ]),
    )?;
    tx.commit()?;
    Ok(node_id)
}

/// Seed one property node (without edges). Returns the NodeId.
fn seed_property_node(
    db: &GraphDb,
    p: &OntologyProperty,
    classes: &[OntologyClass],
) -> Result<NodeId, SoError> {
    let owner_class = classes
        .iter()
        .find(|c| c.name == p.owner_name)
        .ok_or_else(|| SoError::UnknownSymbol {
            name: p.owner_name.clone(),
            kind: "class".to_string(),
            valid: classes.iter().map(|c| c.name.clone()).collect(),
            closest_match: None,
            suggestion: None,
        })?;

    let datatype_str = property_type_str(&p.datatype);
    let mut tx = db.begin_write()?;
    let node_id = tx.merge_node(
        PROPERTY_LABEL,
        props(&[
            ("symbol_id", sv(&p.symbol_id)),
            ("name", sv(&p.name)),
            ("datatype", sv(datatype_str)),
            ("required", bv(p.required)),
            ("owner_symbol_id", sv(&owner_class.symbol_id)),
            ("owner_kind", sv("class")),
            ("created_at", iv(p.created_at)),
        ]),
    )?;
    tx.commit()?;
    Ok(node_id)
}

fn property_type_str(t: &PropertyType) -> &'static str {
    match t {
        PropertyType::String => "string",
        PropertyType::Int64 => "int64",
        PropertyType::Float64 => "float64",
        PropertyType::Bool => "bool",
        PropertyType::Date => "date",
        PropertyType::Variant => "variant",
    }
}

// ── Schema extension operations ───────────────────────────────────────────────

/// Create a `__SO_SUBCLASS_OF` edge from `child` class to `parent` class.
///
/// Cycle detection is performed first.
pub fn define_subclass(db: &GraphDb, child: &str, parent: &str) -> Result<(), SoError> {
    let child_sym = resolve(db, child, AliasKind::Class)?;
    let parent_sym = resolve(db, parent, AliasKind::Class)?;

    // Cycle detection
    check_no_cycle(
        db,
        &child_sym.canonical_name,
        &parent_sym.canonical_name,
        SUBCLASS_OF_REL,
    )?;

    // Create edge using WriteTx (bypasses reserved rel-type check)
    let child_node_id = get_class_node_id(db, &child_sym.canonical_name)?;
    let parent_node_id = get_class_node_id(db, &parent_sym.canonical_name)?;

    let mut tx = db.begin_write()?;
    tx.create_edge(child_node_id, parent_node_id, SUBCLASS_OF_REL, HashMap::new())?;
    tx.commit()?;
    Ok(())
}

/// Register an alias for a canonical class or relation.
///
/// Returns `SoError::AliasConflict` if `alias_name` is already registered
/// for a DIFFERENT canonical target of the same kind.
/// Same alias name for different kinds (class vs relation) is NOT a conflict.
pub fn add_alias(
    db: &GraphDb,
    alias_name: &str,
    kind: AliasKind,
    target_canonical: &str,
) -> Result<(), SoError> {
    let target = resolve(db, target_canonical, kind.clone())?;

    let kind_str = crate::resolution::alias_kind_str(&kind);
    let safe_alias = escape_cypher_string(alias_name);

    // Check for existing alias of same name + kind
    let q = format!(
        "MATCH (a:{ALIAS_LABEL})-[:{ALIAS_OF_REL}]->(existing) \
         WHERE a.name = '{safe_alias}' AND a.kind = '{kind_str}' \
         RETURN existing.name"
    );
    let result = match db.execute(&q) {
        Ok(r) => r,
        Err(sparrowdb_common::Error::InvalidArgument(ref msg))
            if msg.contains("unknown label") =>
        {
            sparrowdb_execution::QueryResult::empty(vec![])
        }
        Err(e) => return Err(SoError::Storage(e)),
    };

    if let Some(row) = result.rows.first() {
        if let Some(ExecValue::String(existing_name)) = row.first() {
            if existing_name != &target.canonical_name {
                return Err(SoError::AliasConflict {
                    alias: alias_name.to_string(),
                    existing: existing_name.clone(),
                    kind: kind_str.to_string(),
                });
            }
            // Already registered for same target — idempotent
            return Ok(());
        }
    }

    // Create the alias node
    let alias_id = uuid::Uuid::new_v4().to_string();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("time")
        .as_millis() as i64;

    let canonical_label = match kind {
        AliasKind::Class => CLASS_LABEL,
        AliasKind::Relation => RELATION_LABEL,
    };

    // Get the target node ID to link the ALIAS_OF edge
    let target_node_id = get_node_id_by_symbol_id(db, canonical_label, &target.symbol_id)?;

    // Create alias node + ALIAS_OF edge
    let mut tx = db.begin_write()?;
    let alias_node_id = tx.merge_node(
        ALIAS_LABEL,
        props(&[
            ("symbol_id", sv(&alias_id)),
            ("name", sv(alias_name)),
            ("kind", sv(kind_str)),
            ("target_name", sv(&target.canonical_name)),
            ("created_at", iv(now)),
        ]),
    )?;
    tx.create_edge(alias_node_id, target_node_id, ALIAS_OF_REL, HashMap::new())?;
    tx.commit()?;

    Ok(())
}

/// Add a property declaration to an existing class or relation.
///
/// - `owner`: canonical or alias name of the class that owns this property.
/// - `prop_name`: the property key (must not start with `__so_`).
/// - `datatype`: one of `string`, `int64`, `float64`, `bool`, `date`, `variant`.
/// - `required`: whether the property must be present on create.
///
/// Returns `SoError::DuplicateProperty` if a property with this name is already
/// declared on the resolved class.
pub fn add_property(
    db: &GraphDb,
    owner: &str,
    prop_name: &str,
    datatype_str: &str,
    required: bool,
    unique: bool,
    allowed_values: Option<Vec<String>>,
) -> Result<OntologyProperty, SoError> {
    // Guard: reserved key prefix
    if prop_name.starts_with("__so_") || prop_name.starts_with("__SO_") {
        return Err(SoError::ReservedProperty(prop_name.to_string()));
    }

    // Resolve owner class
    let class_sym = resolve(db, owner, AliasKind::Class)?;

    // Parse datatype
    let datatype = parse_property_type_str(datatype_str);

    // Check for duplicate
    let safe_sid = escape_cypher_string(&class_sym.symbol_id);
    let safe_pname = escape_cypher_string(prop_name);
    let dup_q = format!(
        "MATCH (c:{CLASS_LABEL} {{symbol_id: '{safe_sid}'}})-[:{HAS_PROPERTY_REL}]->(p:{PROPERTY_LABEL} {{name: '{safe_pname}'}}) \
         RETURN p.symbol_id"
    );
    let dup_result = match db.execute(&dup_q) {
        Ok(r) => r,
        Err(sparrowdb_common::Error::InvalidArgument(ref msg))
            if msg.contains("unknown label") || msg.contains("unknown relationship type") =>
        {
            sparrowdb_execution::QueryResult::empty(vec![])
        }
        Err(e) => return Err(SoError::Storage(e)),
    };
    if !dup_result.rows.is_empty() {
        return Err(SoError::DuplicateProperty {
            class: class_sym.canonical_name.clone(),
            property: prop_name.to_string(),
        });
    }

    // Seed property node
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("time")
        .as_millis() as i64;
    let symbol_id = uuid::Uuid::new_v4().to_string();
    let datatype_label = property_type_str(&datatype);
    let enum_json = allowed_values
        .as_deref()
        .map(|v| serde_json::to_string(v).unwrap_or_default())
        .unwrap_or_default();

    let mut tx = db.begin_write()?;
    let prop_node_id = tx.merge_node(
        PROPERTY_LABEL,
        props(&[
            ("symbol_id", sv(&symbol_id)),
            ("name", sv(prop_name)),
            ("datatype", sv(datatype_label)),
            ("required", bv(required)),
            ("unique", bv(unique)),
            ("enum_values", sv(&enum_json)),
            ("owner_symbol_id", sv(&class_sym.symbol_id)),
            ("owner_kind", sv("class")),
            ("created_at", iv(now)),
        ]),
    )?;
    tx.commit()?;

    // Create HAS_PROPERTY edge: class → property
    let class_node_id = get_class_node_id(db, &class_sym.canonical_name)?;
    let mut tx2 = db.begin_write()?;
    tx2.create_edge(class_node_id, prop_node_id, HAS_PROPERTY_REL, HashMap::new())?;
    tx2.commit()?;

    // Emit uniqueness constraint if requested
    if unique {
        let safe_class = escape_cypher_string(&class_sym.canonical_name);
        let safe_prop = escape_cypher_string(prop_name);
        let constraint_q = format!(
            "CREATE CONSTRAINT ON (n:{safe_class}) ASSERT n.{safe_prop} IS UNIQUE"
        );
        db.execute(&constraint_q).map_err(SoError::Storage)?;
    }

    Ok(OntologyProperty {
        symbol_id,
        name: prop_name.to_string(),
        datatype,
        required,
        unique,
        allowed_values,
        default_value: None,
        owner_symbol_id: class_sym.symbol_id,
        owner_kind: crate::model::OwnerKind::Class,
        created_at: now,
        owner_name: class_sym.canonical_name,
    })
}

fn parse_property_type_str(s: &str) -> PropertyType {
    match s {
        "string" => PropertyType::String,
        "int64" => PropertyType::Int64,
        "float64" => PropertyType::Float64,
        "bool" => PropertyType::Bool,
        "date" => PropertyType::Date,
        _ => PropertyType::Variant,
    }
}

// ── NodeId lookup helpers ─────────────────────────────────────────────────────

/// Get the NodeId for a __SO_Class by canonical name.
fn get_class_node_id(db: &GraphDb, name: &str) -> Result<NodeId, SoError> {
    get_node_id_by_name(db, CLASS_LABEL, name)
}

/// Get the NodeId for a node by label and name property.
fn get_node_id_by_name(db: &GraphDb, label: &str, name: &str) -> Result<NodeId, SoError> {
    let safe_name = escape_cypher_string(name);
    let q = format!("MATCH (n:{label} {{name: '{safe_name}'}}) RETURN id(n)");
    let result = db.execute(&q)?;
    result
        .rows
        .first()
        .and_then(|r| r.first())
        .and_then(|v| {
            if let ExecValue::Int64(n) = v {
                Some(NodeId(*n as u64))
            } else {
                None
            }
        })
        .ok_or_else(|| SoError::UnknownSymbol {
            name: name.to_string(),
            kind: label.to_string(),
            valid: vec![],
            closest_match: None,
            suggestion: None,
        })
}

/// Get the NodeId for a node by label and symbol_id.
fn get_node_id_by_symbol_id(
    db: &GraphDb,
    label: &str,
    symbol_id: &str,
) -> Result<NodeId, SoError> {
    let safe_sid = escape_cypher_string(symbol_id);
    let q = format!("MATCH (n:{label} {{symbol_id: '{safe_sid}'}}) RETURN id(n)");
    let result = db.execute(&q)?;
    result
        .rows
        .first()
        .and_then(|r| r.first())
        .and_then(|v| {
            if let ExecValue::Int64(n) = v {
                Some(NodeId(*n as u64))
            } else {
                None
            }
        })
        .ok_or_else(|| SoError::Storage(sparrowdb_common::Error::NotFound))
}
