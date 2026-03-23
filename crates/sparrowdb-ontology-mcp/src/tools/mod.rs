pub mod data;
pub mod schema;

use serde_json::{json, Value};
use sparrowdb::GraphDb;

use crate::error::mcp_error;

pub fn handle_tool_call(
    db: &GraphDb,
    name: &str,
    params: Option<Value>,
) -> Result<Value, Value> {
    match name {
        // Schema / ontology-definition tools
        "start_here"
        | "get_ontology"
        | "define_class"
        | "define_relation"
        | "add_alias"
        | "add_property"
        | "define_subclass"
        | "define_subproperty"
        | "resolve_name"
        | "health"
        | "stats" => schema::dispatch(db, name, params),

        // Data / entity tools
        "create_entity"
        | "create_relationship"
        | "update_entity"
        | "find_entities"
        | "explain_symbol"
        | "validate" => data::dispatch(db, name, params),

        other => Err(mcp_error(
            -32601,
            "Method not found",
            json!({"tool": other}),
        )),
    }
}
