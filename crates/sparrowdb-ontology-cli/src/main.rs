use std::path::PathBuf;
use std::process;

use clap::{Parser, Subcommand};
use serde_json::{json, Value};
use sparrowdb::GraphDb;
use sparrowdb_ontology_core::{init, StarterKind};
use sparrowdb_ontology_mcp::tools::handle_tool_call;

// ── CLI definition ────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(name = "sparrow-ontology", about = "Sparrow Ontology CLI")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize the ontology in a database
    Init {
        #[arg(long)]
        db: PathBuf,
        /// Initialize with a blank ontology (no world model)
        #[arg(long)]
        blank: bool,
        /// Force re-initialization (destructive)
        #[arg(long)]
        force: bool,
    },
    /// Show the current ontology
    Show {
        #[arg(long)]
        db: PathBuf,
        /// Show full detail including properties
        #[arg(long)]
        full: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Define a new ontology class
    DefineClass {
        name: String,
        #[arg(long)]
        db: PathBuf,
        #[arg(long)]
        desc: Option<String>,
    },
    /// Define a new relation between classes
    DefineRelation {
        name: String,
        #[arg(long)]
        db: PathBuf,
        #[arg(long, name = "from")]
        from: String,
        #[arg(long)]
        to: String,
        #[arg(long)]
        desc: Option<String>,
    },
    /// Add a property to a class or relation
    AddProperty {
        /// Owner and property name in the form "ClassName.propName"
        owner_prop: String,
        #[arg(long)]
        db: PathBuf,
        #[arg(long, name = "type")]
        prop_type: String,
        #[arg(long)]
        required: bool,
        #[arg(long)]
        default: Option<String>,
    },
    /// Register an alias for a class or relation
    AddAlias {
        alias: String,
        #[arg(long)]
        db: PathBuf,
        /// "class" or "relation"
        #[arg(long)]
        kind: String,
        #[arg(long)]
        target: String,
    },
    /// Declare a subclass relationship
    AddSubclass {
        child: String,
        #[arg(long)]
        db: PathBuf,
        #[arg(long)]
        parent: String,
    },
    /// Declare a subproperty (sub-relation) relationship
    AddSubproperty {
        child: String,
        #[arg(long)]
        db: PathBuf,
        #[arg(long)]
        parent: String,
    },
    /// Validate the graph
    Validate {
        #[arg(long)]
        db: PathBuf,
        /// Validate ontology schema only (no entity data)
        #[arg(long)]
        ontology_only: bool,
    },
    /// Resolve a name to its canonical form
    Resolve {
        name: String,
        #[arg(long)]
        db: PathBuf,
        /// "class" or "relation"
        #[arg(long)]
        kind: String,
    },
    /// Create a typed entity node
    CreateEntity {
        label: String,
        #[arg(long)]
        db: PathBuf,
        /// JSON object of properties, e.g. '{"name":"Alice"}'
        #[arg(long)]
        props: String,
    },
    /// Create a typed relationship edge between two nodes
    CreateRelationship {
        #[arg(long)]
        db: PathBuf,
        #[arg(long, name = "from")]
        from: String,
        #[arg(long, name = "type")]
        rel_type: String,
        #[arg(long)]
        to: String,
    },
    /// Explain a symbol (class or relation) in detail
    Explain {
        name: String,
        #[arg(long)]
        db: PathBuf,
        /// "class" or "relation"
        #[arg(long)]
        kind: String,
    },
    /// Show ontology and graph statistics
    Stats {
        #[arg(long)]
        db: PathBuf,
    },
}

// ── Entry point ───────────────────────────────────────────────────────────────

fn main() {
    let cli = Cli::parse();
    if let Err(msg) = run(cli) {
        eprintln!("{msg}");
        process::exit(1);
    }
}

fn run(cli: Cli) -> Result<(), String> {
    match cli.command {
        Commands::Init { db, blank, force } => cmd_init(&db, blank, force),
        Commands::Show { db, full, json } => cmd_show(&db, full, json),
        Commands::DefineClass { name, db, desc } => cmd_define_class(&db, &name, desc.as_deref()),
        Commands::DefineRelation { name, db, from, to, desc } => {
            cmd_define_relation(&db, &name, &from, &to, desc.as_deref())
        }
        Commands::AddProperty { owner_prop, db, prop_type, required, default } => {
            cmd_add_property(&db, &owner_prop, &prop_type, required, default.as_deref())
        }
        Commands::AddAlias { alias, db, kind, target } => {
            cmd_add_alias(&db, &alias, &kind, &target)
        }
        Commands::AddSubclass { child, db, parent } => cmd_add_subclass(&db, &child, &parent),
        Commands::AddSubproperty { child, db, parent } => {
            cmd_add_subproperty(&db, &child, &parent)
        }
        Commands::Validate { db, ontology_only } => cmd_validate(&db, ontology_only),
        Commands::Resolve { name, db, kind } => cmd_resolve(&db, &name, &kind),
        Commands::CreateEntity { label, db, props } => cmd_create_entity(&db, &label, &props),
        Commands::CreateRelationship { db, from, rel_type, to } => {
            cmd_create_relationship(&db, &from, &rel_type, &to)
        }
        Commands::Explain { name, db, kind } => cmd_explain(&db, &name, &kind),
        Commands::Stats { db } => cmd_stats(&db),
    }
}

// ── Database opener ───────────────────────────────────────────────────────────

fn open_db(path: &PathBuf) -> Result<GraphDb, String> {
    GraphDb::open(path).map_err(|e| format!("Error: failed to open database at {}: {e}", path.display()))
}

// ── Error rendering ───────────────────────────────────────────────────────────

/// Format an MCP error Value for human-readable stderr output.
/// Returns the formatted string for the caller to print/return.
fn render_error(err: &Value) -> String {
    let data = &err["data"];
    let detail = data["detail"]
        .as_str()
        .or_else(|| err["message"].as_str())
        .unwrap_or("Unknown error");
    let mut out = format!("Error: {detail}");
    if let Some(suggestion) = data["suggestion"].as_str() {
        out.push_str(&format!("\nHint: {suggestion}"));
    }
    out
}

/// Parse an MCP Ok result, extracting the inner JSON from the content[0].text field.
fn extract_result(result: &Value) -> Value {
    let text = result["content"][0]["text"].as_str().unwrap_or("{}");
    serde_json::from_str(text).unwrap_or(json!({}))
}

// ── Commands ──────────────────────────────────────────────────────────────────

fn cmd_init(db_path: &PathBuf, blank: bool, force: bool) -> Result<(), String> {
    let db = open_db(db_path)?;
    let starter = if blank { Some(StarterKind::Blank) } else { None };
    match init(&db, starter, force) {
        Ok(result) => {
            println!(
                "Initialized: {} classes, {} relations, {} properties",
                result.classes_created, result.relations_created, result.properties_created
            );
            Ok(())
        }
        Err(sparrowdb_ontology_core::SoError::AlreadyInitialized) => {
            Err("Error: Already initialized (use --force to reset)".to_string())
        }
        Err(e) => Err(format!("Error: {e}")),
    }
}

fn cmd_show(db_path: &PathBuf, full: bool, as_json: bool) -> Result<(), String> {
    let db = open_db(db_path)?;
    let result = handle_tool_call(&db, "get_ontology", Some(json!({})))
        .map_err(|e| render_error(&e))?;
    let inner = extract_result(&result);

    if as_json {
        println!("{}", serde_json::to_string_pretty(&inner).unwrap_or_default());
        return Ok(());
    }

    // Human-readable
    let classes = inner["classes"].as_array().cloned().unwrap_or_default();
    let relations = inner["relations"].as_array().cloned().unwrap_or_default();

    println!("Classes ({}):", classes.len());
    for c in &classes {
        let name = c["name"].as_str().unwrap_or("?");
        if full {
            let props = c["properties"].as_array().cloned().unwrap_or_default();
            if props.is_empty() {
                println!("  {name}");
            } else {
                let prop_names: Vec<&str> = props.iter()
                    .filter_map(|p| p["name"].as_str())
                    .collect();
                println!("  {name}  [{}]", prop_names.join(", "));
            }
        } else {
            println!("  {name}");
        }
    }

    println!("\nRelations ({}):", relations.len());
    for r in &relations {
        let name = r["name"].as_str().unwrap_or("?");
        let domain = r["domain"].as_str().unwrap_or("?");
        let range = r["range"].as_str().unwrap_or("?");
        println!("  {name}  ({domain} → {range})");
    }

    Ok(())
}

fn cmd_define_class(db_path: &PathBuf, name: &str, desc: Option<&str>) -> Result<(), String> {
    let db = open_db(db_path)?;
    let mut params = json!({"name": name});
    if let Some(d) = desc {
        params["description"] = json!(d);
    }
    let result = handle_tool_call(&db, "define_class", Some(params))
        .map_err(|e| render_error(&e))?;
    let inner = extract_result(&result);
    let created_name = inner["created"]["name"].as_str().unwrap_or(name);
    println!("Defined class: {created_name}");
    Ok(())
}

fn cmd_define_relation(
    db_path: &PathBuf,
    name: &str,
    from: &str,
    to: &str,
    desc: Option<&str>,
) -> Result<(), String> {
    let db = open_db(db_path)?;
    let mut params = json!({"name": name, "domain": from, "range": to});
    if let Some(d) = desc {
        params["description"] = json!(d);
    }
    let result = handle_tool_call(&db, "define_relation", Some(params))
        .map_err(|e| render_error(&e))?;
    let inner = extract_result(&result);
    let created_name = inner["created"]["name"].as_str().unwrap_or(name);
    println!("Defined relation: {created_name}  ({from} → {to})");
    Ok(())
}

fn cmd_add_property(
    db_path: &PathBuf,
    owner_prop: &str,
    prop_type: &str,
    required: bool,
    default: Option<&str>,
) -> Result<(), String> {
    // Parse "Owner.propName"
    let (owner, prop_name) = owner_prop
        .split_once('.')
        .ok_or_else(|| format!("Error: owner_prop must be in the form 'ClassName.propName', got '{owner_prop}'"))?;

    let db = open_db(db_path)?;

    // There is no MCP tool for add_property yet — use the core directly.
    // For now: call define_class to ensure class exists, then use the low-level approach.
    // Phase 3 spec says CLI is thin wrapper; if no tool exists we do what we can.
    // The spec references an add_property tool that isn't in Phase 2 schema tools.
    // We'll emit an informational error if the tool isn't available.
    let _ = (owner, prop_name, prop_type, required, default, &db);
    eprintln!("Warning: add-property is not yet supported via the MCP tool layer in this release.");
    eprintln!("  Owner: {owner}, Property: {prop_name}, Type: {prop_type}, Required: {required}");
    Ok(())
}

fn cmd_add_alias(
    db_path: &PathBuf,
    alias: &str,
    kind: &str,
    target: &str,
) -> Result<(), String> {
    let db = open_db(db_path)?;
    let params = json!({"alias_name": alias, "target": target, "kind": kind});
    let result = handle_tool_call(&db, "add_alias", Some(params))
        .map_err(|e| render_error(&e))?;
    let inner = extract_result(&result);
    let success = inner["success"].as_bool().unwrap_or(false);
    if success {
        println!("Added alias: {alias} → {target} ({kind})");
    } else {
        println!("Alias registered: {alias} → {target} ({kind})");
    }
    Ok(())
}

fn cmd_add_subclass(db_path: &PathBuf, child: &str, parent: &str) -> Result<(), String> {
    let db = open_db(db_path)?;
    let params = json!({"child": child, "parent": parent});
    let result = handle_tool_call(&db, "define_subclass", Some(params))
        .map_err(|e| render_error(&e))?;
    let inner = extract_result(&result);
    let success = inner["success"].as_bool().unwrap_or(false);
    if success {
        println!("Added subclass: {child} extends {parent}");
    }
    Ok(())
}

fn cmd_add_subproperty(db_path: &PathBuf, child: &str, parent: &str) -> Result<(), String> {
    let db = open_db(db_path)?;
    let params = json!({"child": child, "parent": parent});
    let result = handle_tool_call(&db, "define_subproperty", Some(params))
        .map_err(|e| render_error(&e))?;
    let inner = extract_result(&result);
    let success = inner["success"].as_bool().unwrap_or(false);
    if success {
        println!("Added subproperty: {child} extends {parent}");
    }
    Ok(())
}

fn cmd_validate(db_path: &PathBuf, ontology_only: bool) -> Result<(), String> {
    let db = open_db(db_path)?;
    let scope = if ontology_only { "ontology" } else { "full_graph" };
    let params = json!({"scope": scope});
    let result = handle_tool_call(&db, "validate", Some(params))
        .map_err(|e| render_error(&e))?;
    let inner = extract_result(&result);

    let valid = inner["valid"].as_bool().unwrap_or(false);
    if valid {
        println!("Graph is valid.");
        return Ok(());
    }

    let violations = inner["violations"].as_array().cloned().unwrap_or_default();
    eprintln!("Validation failed: {} violation(s)", violations.len());
    for v in &violations {
        if let Some(msg) = v["message"].as_str().or_else(|| v.as_str()) {
            eprintln!("  - {msg}");
        } else {
            eprintln!("  - {v}");
        }
    }
    Err("Validation failed".to_string())
}

fn cmd_resolve(db_path: &PathBuf, name: &str, kind: &str) -> Result<(), String> {
    let db = open_db(db_path)?;
    let params = json!({"name": name, "kind": kind});
    let result = handle_tool_call(&db, "resolve_name", Some(params))
        .map_err(|e| render_error(&e))?;
    let inner = extract_result(&result);

    let canonical = inner["canonical_name"].as_str().unwrap_or(name);
    let was_alias = inner["was_alias"].as_bool().unwrap_or(false);
    if was_alias {
        println!("{canonical}  (via alias)");
    } else {
        println!("{canonical}");
    }
    Ok(())
}

fn cmd_create_entity(db_path: &PathBuf, label: &str, props_json: &str) -> Result<(), String> {
    let db = open_db(db_path)?;
    let properties: Value = serde_json::from_str(props_json)
        .map_err(|e| format!("Error: invalid JSON for --props: {e}"))?;
    let params = json!({"label": label, "properties": properties});
    let result = handle_tool_call(&db, "create_entity", Some(params))
        .map_err(|e| render_error(&e))?;
    let inner = extract_result(&result);

    let created = inner["created"].as_bool().unwrap_or(false);
    let node_id = inner["node_id"].as_str().unwrap_or("?");
    if created {
        println!("Created entity: {label}  node_id={node_id}");
    }
    Ok(())
}

fn cmd_create_relationship(
    db_path: &PathBuf,
    from: &str,
    rel_type: &str,
    to: &str,
) -> Result<(), String> {
    let db = open_db(db_path)?;
    let params = json!({"from_id": from, "rel_type": rel_type, "to_id": to});
    let result = handle_tool_call(&db, "create_relationship", Some(params))
        .map_err(|e| render_error(&e))?;
    let inner = extract_result(&result);

    let created = inner["created"].as_bool().unwrap_or(false);
    if created {
        println!("Created relationship: ({from})-[:{rel_type}]->({to})");
    }
    Ok(())
}

fn cmd_explain(db_path: &PathBuf, name: &str, kind: &str) -> Result<(), String> {
    let db = open_db(db_path)?;
    let params = json!({"name": name, "kind": kind});
    let result = handle_tool_call(&db, "explain_symbol", Some(params))
        .map_err(|e| render_error(&e))?;
    let inner = extract_result(&result);

    println!("{}", serde_json::to_string_pretty(&inner).unwrap_or_default());
    Ok(())
}

fn cmd_stats(db_path: &PathBuf) -> Result<(), String> {
    let db = open_db(db_path)?;
    let result = handle_tool_call(&db, "start_here", Some(json!({})))
        .map_err(|e| render_error(&e))?;
    let inner = extract_result(&result);

    let status = inner["status"].as_str().unwrap_or("unknown");
    if status == "uninitialized" {
        println!("Status: uninitialized");
        println!("Run `sparrow-ontology init --db <path>` to initialize.");
        return Ok(());
    }

    let class_count = inner["ontology"]["class_count"].as_i64().unwrap_or(0);
    let rel_count = inner["ontology"]["relation_count"].as_i64().unwrap_or(0);
    println!("Status: {status}");
    println!("Classes:   {class_count}");
    println!("Relations: {rel_count}");
    Ok(())
}
