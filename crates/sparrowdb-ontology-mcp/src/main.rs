use sparrowdb_ontology_mcp::tools;

use clap::{Parser, ValueEnum};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

// ── CLI args ──────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, ValueEnum)]
enum Transport {
    Stdio,
    Http,
}

#[derive(Parser, Debug)]
#[command(name = "sparrow-ontology-mcp")]
#[command(about = "MCP server for Sparrow Ontology — ontology-aware semantic layer over SparrowDB")]
#[command(version)]
struct Args {
    /// Path to the SparrowDB database directory
    #[arg(long)]
    db: PathBuf,

    /// Transport mode: stdio (default, for Claude Desktop/Code) or http
    #[arg(long, default_value = "stdio")]
    transport: Transport,

    /// Port to listen on when --transport http is selected
    #[arg(long, default_value = "3456")]
    port: u16,
}

// ── JSON-RPC types ────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    id: Option<Value>,
    method: String,
    params: Option<Value>,
}

#[derive(Serialize)]
struct JsonRpcResponse {
    jsonrpc: String,
    id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<Value>,
}

// ── Entry point ───────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    let args = Args::parse();

    let db = match sparrowdb::GraphDb::open(&args.db) {
        Ok(db) => db,
        Err(e) => {
            eprintln!("Failed to open database at {:?}: {e}", args.db);
            std::process::exit(1);
        }
    };

    match args.transport {
        Transport::Stdio => run_stdio(db),
        Transport::Http => run_http(db, args.port).await,
    }
}

// ── stdio transport ───────────────────────────────────────────────────────────

fn run_stdio(db: sparrowdb::GraphDb) {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut out = stdout.lock();

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        if line.is_empty() {
            continue;
        }

        let response = match serde_json::from_str::<JsonRpcRequest>(&line) {
            Ok(req) => handle_request(&db, req),
            Err(e) => JsonRpcResponse {
                jsonrpc: "2.0".into(),
                id: None,
                result: None,
                error: Some(json!({"code": -32700, "message": e.to_string()})),
            },
        };

        let resp_str = match serde_json::to_string(&response) {
            Ok(s) => s,
            Err(_) => continue,
        };
        if writeln!(out, "{resp_str}").is_err() {
            break;
        }
        if out.flush().is_err() {
            break;
        }
    }
}

// ── HTTP transport ────────────────────────────────────────────────────────────

type SharedDb = Arc<Mutex<sparrowdb::GraphDb>>;

async fn run_http(db: sparrowdb::GraphDb, port: u16) {
    use axum::{
        extract::State,
        http::StatusCode,
        response::{IntoResponse, Response},
        routing::{get, post},
        Router,
    };

    let shared = Arc::new(Mutex::new(db));

    async fn health_endpoint(State(db): State<SharedDb>) -> impl IntoResponse {
        let guard = match db.lock() {
            Ok(g) => g,
            Err(_) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    axum::Json(json!({"error": "db lock poisoned"})),
                )
                    .into_response();
            }
        };
        match tools::handle_tool_call(&guard, "health", None) {
            Ok(result) => {
                let text = result["content"][0]["text"].as_str().unwrap_or("{}");
                let payload: Value = serde_json::from_str(text).unwrap_or(json!({}));
                (StatusCode::OK, axum::Json(payload)).into_response()
            }
            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, axum::Json(e)).into_response(),
        }
    }

    async fn stats_endpoint(State(db): State<SharedDb>) -> impl IntoResponse {
        let guard = match db.lock() {
            Ok(g) => g,
            Err(_) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    axum::Json(json!({"error": "db lock poisoned"})),
                )
                    .into_response();
            }
        };
        match tools::handle_tool_call(&guard, "stats", None) {
            Ok(result) => {
                let text = result["content"][0]["text"].as_str().unwrap_or("{}");
                let payload: Value = serde_json::from_str(text).unwrap_or(json!({}));
                (StatusCode::OK, axum::Json(payload)).into_response()
            }
            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, axum::Json(e)).into_response(),
        }
    }

    async fn mcp_endpoint(State(db): State<SharedDb>, body: String) -> Response {
        let req = match serde_json::from_str::<JsonRpcRequest>(&body) {
            Ok(r) => r,
            Err(e) => {
                let resp = JsonRpcResponse {
                    jsonrpc: "2.0".into(),
                    id: None,
                    result: None,
                    error: Some(json!({"code": -32700, "message": e.to_string()})),
                };
                return (
                    StatusCode::OK,
                    axum::Json(serde_json::to_value(resp).unwrap()),
                )
                    .into_response();
            }
        };

        let resp = {
            let guard = match db.lock() {
                Ok(g) => g,
                Err(_) => {
                    let resp = JsonRpcResponse {
                        jsonrpc: "2.0".into(),
                        id: req.id,
                        result: None,
                        error: Some(
                            json!({"code": -32603, "message": "Internal error: db lock poisoned"}),
                        ),
                    };
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        axum::Json(serde_json::to_value(resp).unwrap()),
                    )
                        .into_response();
                }
            };
            handle_request(&guard, req)
        };

        (
            StatusCode::OK,
            axum::Json(serde_json::to_value(resp).unwrap()),
        )
            .into_response()
    }

    let app = Router::new()
        .route("/health", get(health_endpoint))
        .route("/ontology/stats", get(stats_endpoint))
        .route("/mcp", post(mcp_endpoint))
        .with_state(shared);

    let addr = format!("0.0.0.0:{port}");
    let listener = match tokio::net::TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Failed to bind to {addr}: {e}");
            std::process::exit(1);
        }
    };
    eprintln!("sparrow-ontology-mcp listening on http://{addr}");
    eprintln!("  POST /mcp            — JSON-RPC endpoint");
    eprintln!("  GET  /health         — health check");
    eprintln!("  GET  /ontology/stats — ontology analytics");
    if let Err(e) = axum::serve(listener, app).await {
        eprintln!("Server error: {e}");
        std::process::exit(1);
    }
}

// ── Request dispatcher ────────────────────────────────────────────────────────

fn handle_request(db: &sparrowdb::GraphDb, req: JsonRpcRequest) -> JsonRpcResponse {
    if req.jsonrpc != "2.0" {
        return JsonRpcResponse {
            jsonrpc: "2.0".into(),
            id: req.id,
            result: None,
            error: Some(
                json!({"code": -32600, "message": "Invalid Request: jsonrpc must be \"2.0\""}),
            ),
        };
    }

    let result = match req.method.as_str() {
        "initialize" => Ok(json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {"tools": {}},
            "serverInfo": {
                "name": "sparrowdb-ontology-mcp",
                "version": env!("CARGO_PKG_VERSION")
            }
        })),

        "tools/list" => Ok(tool_list()),

        "tools/call" => handle_tool_call(db, req.params),

        _ => Err(json!({"code": -32601, "message": "Method not found"})),
    };

    match result {
        Ok(r) => JsonRpcResponse {
            jsonrpc: "2.0".into(),
            id: req.id,
            result: Some(r),
            error: None,
        },
        Err(e) => JsonRpcResponse {
            jsonrpc: "2.0".into(),
            id: req.id,
            result: None,
            error: Some(e),
        },
    }
}

// ── tools/call dispatcher ─────────────────────────────────────────────────────

fn handle_tool_call(db: &sparrowdb::GraphDb, params: Option<Value>) -> Result<Value, Value> {
    let params = params.ok_or_else(|| json!({"code": -32602, "message": "Missing params"}))?;
    let tool_name = params["name"]
        .as_str()
        .ok_or_else(|| json!({"code": -32602, "message": "Missing tool name"}))?;
    let args = params.get("arguments").cloned();

    match tools::handle_tool_call(db, tool_name, args) {
        Ok(v) => Ok(v),
        // MCP spec: tool errors must be returned as result with isError:true, NOT as
        // JSON-RPC errors. JSON-RPC errors are swallowed by Claude as "gateway failure"
        // without surfacing the detail. Wrap here so Claude sees the full error.
        Err(e) => {
            let text = {
                let msg = e
                    .get("message")
                    .and_then(|m| m.as_str())
                    .unwrap_or("Tool error");
                let data = e.get("data");
                match data {
                    Some(d) if !d.is_null() => format!(
                        "{msg}\n{}",
                        serde_json::to_string_pretty(d).unwrap_or_default()
                    ),
                    _ => msg.to_string(),
                }
            };
            Ok(json!({
                "content": [{"type": "text", "text": text}],
                "isError": true
            }))
        }
    }
}

// ── tools/list ────────────────────────────────────────────────────────────────

fn tool_list() -> Value {
    json!({
        "tools": [
            {
                "name": "start_here",
                "description": "Check initialization state of the ontology. Returns class/relation/property counts, lists classes with no declared properties (unseeded), and explains the schema-first workflow. Call this first in every session.",
                "inputSchema": {
                    "type": "object",
                    "properties": {},
                    "required": []
                }
            },
            {
                "name": "init",
                "description": "Initialize the ontology schema with a starter template. Creates the initial __SO_Class, __SO_Relation, and __SO_Property nodes.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "starter": {"type": "string", "enum": ["WorldModel", "Blank", "PersonalKnowledge", "ProfessionalNetwork", "ResearchNotes"], "description": "Starter template: Blank (empty), WorldModel (10 classes), or pre-built domains."},
                        "force": {"type": "boolean", "description": "If true, skip the already-initialized check (default: false)"}
                    },
                    "required": []
                }
            },
            {
                "name": "get_ontology",
                "description": "Return the full ontology: all classes, relations, aliases, and properties.",
                "inputSchema": {
                    "type": "object",
                    "properties": {},
                    "required": []
                }
            },
            {
                "name": "define_class",
                "description": "Define a new ontology class (entity type). Name must not start with '__SO_'.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "name": {"type": "string", "description": "Canonical class name"},
                        "description": {"type": "string", "description": "Optional human-readable description"},
                        "iri": {"type": "string", "description": "Optional IRI (e.g. https://schema.org/Person) for JSON-LD export and linked-data integration"}
                    },
                    "required": ["name"]
                }
            },
            {
                "name": "define_relation",
                "description": "Define a new ontology relation with domain and range classes.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "name": {"type": "string", "description": "Canonical relation name (e.g. WORKS_FOR)"},
                        "domain": {"type": "string", "description": "Source class name or alias"},
                        "range": {"type": "string", "description": "Target class name or alias"},
                        "description": {"type": "string", "description": "Optional description"},
                        "directed": {"type": "boolean", "description": "Whether the relation is directed (default: true)"},
                        "iri": {"type": "string", "description": "Optional IRI for JSON-LD export and linked-data integration"}
                    },
                    "required": ["name", "domain", "range"]
                }
            },
            {
                "name": "add_alias",
                "description": "Register a spelling alias for an existing class or relation.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "alias_name": {"type": "string", "description": "The alias spelling to register"},
                        "target": {"type": "string", "description": "The canonical class or relation name"},
                        "kind": {"type": "string", "enum": ["class", "relation"], "description": "Whether the target is a class or relation (default: class)"}
                    },
                    "required": ["alias_name", "target"]
                }
            },
            {
                "name": "add_property",
                "description": "Declare a typed property on an existing class. Enables validate_entity to type-check and require the field.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "owner": {"type": "string", "description": "Class name or alias that owns this property"},
                        "name": {"type": "string", "description": "Property key name (must not start with __so_)"},
                        "datatype": {"type": "string", "enum": ["string", "int64", "float64", "bool", "date", "variant"], "description": "Property value type (default: string)"},
                        "required": {"type": "boolean", "description": "If true, create_entity will reject entities missing this property (default: false)"}
                    },
                    "required": ["owner", "name"]
                }
            },
            {
                "name": "define_subclass",
                "description": "Create a SUBCLASS_OF edge from child class to parent class. Cycle-safe.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "child": {"type": "string", "description": "The subclass (child) name or alias"},
                        "parent": {"type": "string", "description": "The superclass (parent) name or alias"}
                    },
                    "required": ["child", "parent"]
                }
            },
            {
                "name": "define_subproperty",
                "description": "Create a SUBPROPERTY_OF edge from child relation to parent relation. Cycle-safe.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "child": {"type": "string", "description": "The sub-relation (child) name or alias"},
                        "parent": {"type": "string", "description": "The super-relation (parent) name or alias"}
                    },
                    "required": ["child", "parent"]
                }
            },
            {
                "name": "resolve_name",
                "description": "Resolve a name (canonical or alias) to its canonical symbol with full detail.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "name": {"type": "string", "description": "Name or alias to resolve"},
                        "kind": {"type": "string", "enum": ["class", "relation"], "description": "Whether to resolve as class or relation (default: class)"}
                    },
                    "required": ["name"]
                }
            },
            {
                "name": "create_entity",
                "description": "Create a typed entity node validated against the ontology.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "class_name": {"type": "string", "description": "Ontology class name or alias"},
                        "properties": {"type": "object", "description": "Key-value properties for the entity"}
                    },
                    "required": ["class_name"]
                }
            },
            {
                "name": "create_relationship",
                "description": "Create a typed relationship edge between two entities, validated against domain/range.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "from_id": {"type": "string", "description": "Source entity node ID"},
                        "to_id": {"type": "string", "description": "Target entity node ID"},
                        "relation_name": {"type": "string", "description": "Ontology relation name or alias"},
                        "properties": {"type": "object", "description": "Optional edge properties"}
                    },
                    "required": ["from_id", "to_id", "relation_name"]
                }
            },
            {
                "name": "update_entity",
                "description": "Update properties on an existing entity node.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "node_id": {"type": "string", "description": "Entity node ID to update"},
                        "properties": {"type": "object", "description": "Properties to set or update"}
                    },
                    "required": ["node_id", "properties"]
                }
            },
            {
                "name": "find_entities",
                "description": "Find entity nodes by class and optional property filters.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "class_name": {"type": "string", "description": "Ontology class name or alias to search"},
                        "filters": {"type": "object", "description": "Optional property key-value filters"},
                        "limit": {"type": "integer", "description": "Max results (default: 25)"}
                    },
                    "required": ["class_name"]
                }
            },
            {
                "name": "explain_symbol",
                "description": "Explain a class or relation: its properties, aliases, hierarchy, and usage examples.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "name": {"type": "string", "description": "Symbol name or alias to explain"},
                        "kind": {"type": "string", "enum": ["class", "relation"], "description": "Whether to explain as class or relation (default: class)"}
                    },
                    "required": ["name"]
                }
            },
            {
                "name": "validate",
                "description": "Validate an entity or relationship against the ontology schema.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "class_name": {"type": "string", "description": "Class to validate against"},
                        "properties": {"type": "object", "description": "Properties to validate"}
                    },
                    "required": ["class_name"]
                }
            },
            {
                "name": "health",
                "description": "Return operational status of the running server and DB connection. Reports db_connected, class_count, and relation_count.",
                "inputSchema": {
                    "type": "object",
                    "properties": {},
                    "required": []
                }
            },
            {
                "name": "stats",
                "description": "Return ontology analytics: schema counts (classes, relations, properties), unseeded classes, and entity counts per class.",
                "inputSchema": {
                    "type": "object",
                    "properties": {},
                    "required": []
                }
            },
            {
                "name": "export_json_ld",
                "description": "Export the full ontology schema as a JSON-LD document. Returns an owl:Class and owl:ObjectProperty graph with @context, rdfs:label, rdfs:comment, skos:altLabel, rdfs:subClassOf, rdfs:domain, rdfs:range, and so: extension terms.",
                "inputSchema": {
                    "type": "object",
                    "properties": {},
                    "required": []
                }
            },
            {
                "name": "import_turtle",
                "description": "Import an ontology from Turtle (.ttl) format into the database. Accepts raw Turtle text. Returns an import summary with counts of classes, relations, subclasses, and aliases imported, plus any warnings for unsupported constructs.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "turtle": {"type": "string", "description": "The Turtle (.ttl) ontology text to import"},
                        "base_iri": {"type": "string", "description": "Optional base IRI for resolving relative IRIs in the Turtle file"},
                        "strategy": {"type": "string", "enum": ["first", "unconstrained"], "description": "Domain/range strategy when multiple values exist. 'unconstrained' (default) sets no domain/range if multiple exist; 'first' takes the first value.", "default": "unconstrained"}
                    },
                    "required": ["turtle"]
                }
            }
        ]
    })
}
