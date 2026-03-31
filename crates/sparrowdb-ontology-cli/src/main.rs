use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process;

use clap::{Parser, Subcommand};
use serde_json::{json, Value};
use sparrowdb::GraphDb;
use sparrowdb_ontology_core::{
    export_json_ld, import_records, import_turtle, init, DomainRangeStrategy, ImportOptions,
    ImportTemplate, StarterKind,
};
use sparrowdb_ontology_mcp::tools::handle_tool_call;

// ── CLI definition ────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(name = "sparrow-ontology", about = "Sparrow Ontology CLI", version)]
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
        /// Optional IRI (e.g. https://schema.org/Person) for JSON-LD export
        #[arg(long)]
        iri: Option<String>,
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
        /// Optional IRI for JSON-LD export and linked-data integration
        #[arg(long)]
        iri: Option<String>,
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
    /// Export the ontology as a JSON-LD document
    ExportJsonLd {
        #[arg(long)]
        db: PathBuf,
        /// Write output to this file path (default: stdout)
        #[arg(long)]
        output: Option<PathBuf>,
        /// Pretty-print the JSON (2-space indent)
        #[arg(long)]
        pretty: bool,
    },
    /// Import entities from a CSV or JSON file using a mapping template
    Import {
        #[arg(long)]
        db: PathBuf,
        /// Path to the data file (.csv or .json)
        #[arg(long)]
        file: PathBuf,
        /// Path to the JSON mapping template
        #[arg(long)]
        template: PathBuf,
        /// Validate all rows and print what would be created, but don't write
        #[arg(long)]
        dry_run: bool,
        /// Continue on row-level validation errors (log them), don't abort
        #[arg(long)]
        skip_errors: bool,
    },
    /// Import a Turtle (.ttl) ontology file into the database
    ImportTurtle {
        /// Path to the Turtle file to import
        file: PathBuf,
        #[arg(long)]
        db: PathBuf,
        /// Optional base IRI for resolving relative IRIs
        #[arg(long)]
        base_iri: Option<String>,
        /// Domain/range strategy: 'first' (take first value) or 'unconstrained' (ignore if multiple)
        #[arg(long, default_value = "unconstrained")]
        strategy: String,
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
        Commands::DefineClass {
            name,
            db,
            desc,
            iri,
        } => cmd_define_class(&db, &name, desc.as_deref(), iri.as_deref()),
        Commands::DefineRelation {
            name,
            db,
            from,
            to,
            desc,
            iri,
        } => cmd_define_relation(&db, &name, &from, &to, desc.as_deref(), iri.as_deref()),
        Commands::AddProperty {
            owner_prop,
            db,
            prop_type,
            required,
            default,
        } => cmd_add_property(&db, &owner_prop, &prop_type, required, default.as_deref()),
        Commands::AddAlias {
            alias,
            db,
            kind,
            target,
        } => cmd_add_alias(&db, &alias, &kind, &target),
        Commands::AddSubclass { child, db, parent } => cmd_add_subclass(&db, &child, &parent),
        Commands::AddSubproperty { child, db, parent } => cmd_add_subproperty(&db, &child, &parent),
        Commands::Validate { db, ontology_only } => cmd_validate(&db, ontology_only),
        Commands::Resolve { name, db, kind } => cmd_resolve(&db, &name, &kind),
        Commands::CreateEntity { label, db, props } => cmd_create_entity(&db, &label, &props),
        Commands::CreateRelationship {
            db,
            from,
            rel_type,
            to,
        } => cmd_create_relationship(&db, &from, &rel_type, &to),
        Commands::Explain { name, db, kind } => cmd_explain(&db, &name, &kind),
        Commands::Stats { db } => cmd_stats(&db),
        Commands::ExportJsonLd { db, output, pretty } => {
            cmd_export_json_ld(&db, output.as_deref(), pretty)
        }
        Commands::Import {
            db,
            file,
            template,
            dry_run,
            skip_errors,
        } => cmd_import(&db, &file, &template, dry_run, skip_errors),
        Commands::ImportTurtle {
            file,
            db,
            base_iri,
            strategy,
        } => cmd_import_turtle(&db, &file, base_iri, &strategy),
    }
}

// ── Database opener ───────────────────────────────────────────────────────────

fn open_db(path: &Path) -> Result<GraphDb, String> {
    GraphDb::open(path)
        .map_err(|e| format!("Error: failed to open database at {}: {e}", path.display()))
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

fn cmd_init(db_path: &Path, blank: bool, force: bool) -> Result<(), String> {
    let db = open_db(db_path)?;
    let starter = if blank {
        Some(StarterKind::Blank)
    } else {
        None
    };
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

fn cmd_show(db_path: &Path, full: bool, as_json: bool) -> Result<(), String> {
    let db = open_db(db_path)?;
    let result =
        handle_tool_call(&db, "get_ontology", Some(json!({}))).map_err(|e| render_error(&e))?;
    let inner = extract_result(&result);

    if as_json {
        println!(
            "{}",
            serde_json::to_string_pretty(&inner).unwrap_or_default()
        );
        return Ok(());
    }

    // Human-readable
    let classes = inner["classes"]["data"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let relations = inner["relations"]["data"]
        .as_array()
        .cloned()
        .unwrap_or_default();

    println!("Classes ({}):", classes.len());
    for c in &classes {
        let name = c["name"].as_str().unwrap_or("?");
        if full {
            let props = c["properties"].as_array().cloned().unwrap_or_default();
            if props.is_empty() {
                println!("  {name}");
            } else {
                let prop_names: Vec<&str> =
                    props.iter().filter_map(|p| p["name"].as_str()).collect();
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

fn cmd_define_class(
    db_path: &Path,
    name: &str,
    desc: Option<&str>,
    iri: Option<&str>,
) -> Result<(), String> {
    let db = open_db(db_path)?;
    let mut params = json!({"name": name});
    if let Some(d) = desc {
        params["description"] = json!(d);
    }
    if let Some(i) = iri {
        params["iri"] = json!(i);
    }
    let result =
        handle_tool_call(&db, "define_class", Some(params)).map_err(|e| render_error(&e))?;
    let inner = extract_result(&result);
    let created_name = inner["created"]["name"].as_str().unwrap_or(name);
    println!("Defined class: {created_name}");
    Ok(())
}

fn cmd_define_relation(
    db_path: &Path,
    name: &str,
    from: &str,
    to: &str,
    desc: Option<&str>,
    iri: Option<&str>,
) -> Result<(), String> {
    let db = open_db(db_path)?;
    let mut params = json!({"name": name, "domain": from, "range": to});
    if let Some(d) = desc {
        params["description"] = json!(d);
    }
    if let Some(i) = iri {
        params["iri"] = json!(i);
    }
    let result =
        handle_tool_call(&db, "define_relation", Some(params)).map_err(|e| render_error(&e))?;
    let inner = extract_result(&result);
    let created_name = inner["created"]["name"].as_str().unwrap_or(name);
    println!("Defined relation: {created_name}  ({from} → {to})");
    Ok(())
}

fn cmd_add_property(
    db_path: &Path,
    owner_prop: &str,
    prop_type: &str,
    required: bool,
    default: Option<&str>,
) -> Result<(), String> {
    // Parse "Owner.propName"
    let (owner, prop_name) = owner_prop.split_once('.').ok_or_else(|| {
        format!("Error: owner_prop must be in the form 'ClassName.propName', got '{owner_prop}'")
    })?;

    let db = open_db(db_path)?;

    let _ = default; // default_value not yet stored in v1 schema
    let params = json!({
        "owner": owner,
        "name": prop_name,
        "datatype": prop_type,
        "required": required,
    });
    let result =
        handle_tool_call(&db, "add_property", Some(params)).map_err(|e| render_error(&e))?;
    let inner = extract_result(&result);
    if let Some(created) = inner.get("created") {
        let sym = created["symbol_id"].as_str().unwrap_or("?");
        let dt = created["datatype"].as_str().unwrap_or(prop_type);
        let req = created["required"].as_bool().unwrap_or(required);
        println!("Property added: {owner}.{prop_name} ({dt}, required={req}) [{sym}]");
    } else {
        println!("Property added: {owner}.{prop_name}");
    }
    Ok(())
}

fn cmd_add_alias(db_path: &Path, alias: &str, kind: &str, target: &str) -> Result<(), String> {
    let db = open_db(db_path)?;
    let params = json!({"alias_name": alias, "target": target, "kind": kind});
    let result = handle_tool_call(&db, "add_alias", Some(params)).map_err(|e| render_error(&e))?;
    let inner = extract_result(&result);
    let success = inner["success"].as_bool().unwrap_or(false);
    if success {
        println!("Added alias: {alias} → {target} ({kind})");
    } else {
        println!("Alias registered: {alias} → {target} ({kind})");
    }
    Ok(())
}

fn cmd_add_subclass(db_path: &Path, child: &str, parent: &str) -> Result<(), String> {
    let db = open_db(db_path)?;
    let params = json!({"child": child, "parent": parent});
    let result =
        handle_tool_call(&db, "define_subclass", Some(params)).map_err(|e| render_error(&e))?;
    let inner = extract_result(&result);
    let success = inner["success"].as_bool().unwrap_or(false);
    if success {
        println!("Added subclass: {child} extends {parent}");
    }
    Ok(())
}

fn cmd_add_subproperty(db_path: &Path, child: &str, parent: &str) -> Result<(), String> {
    let db = open_db(db_path)?;
    let params = json!({"child": child, "parent": parent});
    let result =
        handle_tool_call(&db, "define_subproperty", Some(params)).map_err(|e| render_error(&e))?;
    let inner = extract_result(&result);
    let success = inner["success"].as_bool().unwrap_or(false);
    if success {
        println!("Added subproperty: {child} extends {parent}");
    }
    Ok(())
}

fn cmd_validate(db_path: &Path, ontology_only: bool) -> Result<(), String> {
    let db = open_db(db_path)?;
    let scope = if ontology_only {
        "ontology"
    } else {
        "full_graph"
    };
    let params = json!({"scope": scope});
    let result = handle_tool_call(&db, "validate", Some(params)).map_err(|e| render_error(&e))?;
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

fn cmd_resolve(db_path: &Path, name: &str, kind: &str) -> Result<(), String> {
    let db = open_db(db_path)?;
    let params = json!({"name": name, "kind": kind});
    let result =
        handle_tool_call(&db, "resolve_name", Some(params)).map_err(|e| render_error(&e))?;
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

fn cmd_create_entity(db_path: &Path, label: &str, props_json: &str) -> Result<(), String> {
    let db = open_db(db_path)?;
    let properties: Value = serde_json::from_str(props_json)
        .map_err(|e| format!("Error: invalid JSON for --props: {e}"))?;
    let params = json!({"label": label, "properties": properties});
    let result =
        handle_tool_call(&db, "create_entity", Some(params)).map_err(|e| render_error(&e))?;
    let inner = extract_result(&result);

    let created = inner["created"].as_bool().unwrap_or(false);
    let node_id = inner["node_id"].as_str().unwrap_or("?");
    if created {
        println!("Created entity: {label}  node_id={node_id}");
    }
    Ok(())
}

fn cmd_create_relationship(
    db_path: &Path,
    from: &str,
    rel_type: &str,
    to: &str,
) -> Result<(), String> {
    let db = open_db(db_path)?;
    let params = json!({"from_id": from, "rel_type": rel_type, "to_id": to});
    let result =
        handle_tool_call(&db, "create_relationship", Some(params)).map_err(|e| render_error(&e))?;
    let inner = extract_result(&result);

    let created = inner["created"].as_bool().unwrap_or(false);
    if created {
        println!("Created relationship: ({from})-[:{rel_type}]->({to})");
    }
    Ok(())
}

fn cmd_explain(db_path: &Path, name: &str, kind: &str) -> Result<(), String> {
    let db = open_db(db_path)?;
    let params = json!({"name": name, "kind": kind});
    let result =
        handle_tool_call(&db, "explain_symbol", Some(params)).map_err(|e| render_error(&e))?;
    let inner = extract_result(&result);

    println!(
        "{}",
        serde_json::to_string_pretty(&inner).unwrap_or_default()
    );
    Ok(())
}

fn cmd_import(
    db_path: &Path,
    file_path: &Path,
    template_path: &Path,
    dry_run: bool,
    skip_errors: bool,
) -> Result<(), String> {
    // Load and parse the JSON template.
    let template_bytes = std::fs::read(template_path).map_err(|e| {
        format!(
            "Error: cannot read template file {}: {e}",
            template_path.display()
        )
    })?;
    let template: ImportTemplate = serde_json::from_slice(&template_bytes)
        .map_err(|e| format!("Error: invalid template JSON: {e}"))?;

    // Detect file format by extension.
    let ext = file_path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();

    let records: Vec<HashMap<String, String>> = match ext.as_str() {
        "csv" => {
            let file = std::fs::File::open(file_path)
                .map_err(|e| format!("Error: cannot open file {}: {e}", file_path.display()))?;
            let mut rdr = csv::Reader::from_reader(file);
            let headers: Vec<String> = rdr
                .headers()
                .map_err(|e| format!("Error: cannot read CSV headers: {e}"))?
                .iter()
                .map(|s| s.to_string())
                .collect();
            let mut rows = Vec::new();
            for (i, result) in rdr.records().enumerate() {
                let row =
                    result.map_err(|e| format!("Error: CSV parse error at row {}: {e}", i + 2))?;
                let map: HashMap<String, String> = headers
                    .iter()
                    .zip(row.iter())
                    .map(|(h, v)| (h.clone(), v.to_string()))
                    .collect();
                rows.push(map);
            }
            rows
        }
        "json" => {
            let bytes = std::fs::read(file_path)
                .map_err(|e| format!("Error: cannot read file {}: {e}", file_path.display()))?;
            let arr: Vec<Value> = serde_json::from_slice(&bytes)
                .map_err(|e| format!("Error: invalid JSON array in file: {e}"))?;
            arr.into_iter()
                .map(|obj| {
                    obj.as_object()
                        .ok_or_else(|| "Error: JSON array must contain objects".to_string())
                        .map(|m| {
                            m.iter()
                                .map(|(k, v)| {
                                    let s = match v {
                                        Value::String(s) => s.clone(),
                                        other => other.to_string(),
                                    };
                                    (k.clone(), s)
                                })
                                .collect::<HashMap<String, String>>()
                        })
                })
                .collect::<Result<Vec<_>, _>>()?
        }
        other => {
            return Err(format!(
                "Error: unsupported file extension '.{other}' — use .csv or .json"
            ));
        }
    };

    let total = records.len();

    if dry_run {
        println!(
            "[dry-run] Validating {total} records against class '{}'...",
            template.class
        );
    }

    let db = open_db(db_path)?;
    let result = import_records(&db, &records, &template, dry_run, skip_errors)
        .map_err(|e| format!("Error: {e}"))?;

    // Print per-row errors.
    for err in &result.errors {
        eprintln!("  Row {}: {}", err.row, err.message);
    }

    // Summary line.
    let action = if dry_run { "Would import" } else { "Imported" };
    println!(
        "{action} {} entities ({} skipped, {} errors)",
        result.created,
        result.skipped,
        result.error_count()
    );

    Ok(())
}

fn cmd_export_json_ld(db_path: &Path, output: Option<&Path>, pretty: bool) -> Result<(), String> {
    let db = open_db(db_path)?;
    let value = export_json_ld(&db).map_err(|e| format!("Error: {e}"))?;
    let json_str = if pretty {
        serde_json::to_string_pretty(&value).map_err(|e| format!("Error: {e}"))?
    } else {
        serde_json::to_string(&value).map_err(|e| format!("Error: {e}"))?
    };
    match output {
        Some(path) => std::fs::write(path, &json_str).map_err(|e| format!("Error: {e}"))?,
        None => println!("{json_str}"),
    }
    Ok(())
}

fn cmd_stats(db_path: &Path) -> Result<(), String> {
    let db = open_db(db_path)?;
    let result =
        handle_tool_call(&db, "start_here", Some(json!({}))).map_err(|e| render_error(&e))?;
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

fn cmd_import_turtle(
    db_path: &Path,
    file: &Path,
    base_iri: Option<String>,
    strategy: &str,
) -> Result<(), String> {
    let ttl = std::fs::read_to_string(file)
        .map_err(|e| format!("Error: cannot read file {}: {e}", file.display()))?;
    let db = open_db(db_path)?;

    let domain_range_strategy = match strategy {
        "first" => DomainRangeStrategy::FirstOnly,
        _ => DomainRangeStrategy::Unconstrained,
    };

    let opts = ImportOptions {
        base_iri,
        domain_range_strategy,
    };
    let summary = import_turtle(&db, &ttl, opts).map_err(|e| format!("import failed: {e}"))?;

    println!("Import complete:");
    println!("  Classes:    {}", summary.classes_imported);
    println!("  Relations:  {}", summary.relations_imported);
    println!("  Subclasses: {}", summary.subclasses_imported);
    println!("  Aliases:    {}", summary.aliases_imported);
    if !summary.warnings.is_empty() {
        println!("Warnings ({}):", summary.warnings.len());
        for w in &summary.warnings {
            println!("  - {w}");
        }
    }
    Ok(())
}
