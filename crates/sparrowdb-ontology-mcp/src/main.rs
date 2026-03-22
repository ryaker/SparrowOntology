use sparrowdb_ontology_mcp::error;
use sparrowdb_ontology_mcp::tools;

use clap::Parser;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};
use std::path::PathBuf;

// ── CLI args ──────────────────────────────────────────────────────────────────

#[derive(Parser, Debug)]
#[command(name = "sparrow-ontology-mcp")]
#[command(about = "MCP server for Sparrow Ontology — ontology-aware semantic layer over SparrowDB")]
#[command(version)]
struct Args {
    /// Path to the SparrowDB database directory
    #[arg(long)]
    db: PathBuf,
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

fn main() {
    let args = Args::parse();

    let db = match sparrowdb::GraphDb::open(&args.db) {
        Ok(db) => db,
        Err(e) => {
            eprintln!("Failed to open database at {:?}: {e}", args.db);
            std::process::exit(1);
        }
    };

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
    let params =
        params.ok_or_else(|| json!({"code": -32602, "message": "Missing params"}))?;
    let tool_name = params["name"]
        .as_str()
        .ok_or_else(|| json!({"code": -32602, "message": "Missing tool name"}))?;
    let args = params.get("arguments").cloned();

    tools::handle_tool_call(db, tool_name, args)
}

// ── tools/list ────────────────────────────────────────────────────────────────

fn tool_list() -> Value {
    json!({
        "tools": [
            {
                "name": "start_here",
                "description": "Check initialization state of the ontology and get orientation on next steps.",
                "inputSchema": {
                    "type": "object",
                    "properties": {},
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
                        "description": {"type": "string", "description": "Optional human-readable description"}
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
                        "directed": {"type": "boolean", "description": "Whether the relation is directed (default: true)"}
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
            }
        ]
    })
}
