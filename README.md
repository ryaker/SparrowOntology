# Sparrow Ontology

**Schema-enforced graph memory for AI agents.**

[![CI](https://github.com/ryaker/SparrowOntology/actions/workflows/ci.yml/badge.svg)](https://github.com/ryaker/SparrowOntology/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

---

Language models are great at writing data into graphs. They're terrible at doing it consistently.

Without a schema, every session your agent decides `Person` is spelled differently, stores email under `email` or `e_mail` or `contact`, and links nodes with whatever relationship name felt right in the moment. You end up with a graph that's full of data and useless for retrieval.

Sparrow Ontology fixes that. It's a typed semantic layer for [SparrowDB](https://github.com/ryaker/SparrowDB) — a pure-Rust embedded graph database. Every write is validated against a schema before it touches the graph. Aliases collapse spelling variants. Domain/range constraints catch broken relationships at write time, not query time. And the whole thing speaks [MCP](https://modelcontextprotocol.io/), so any LLM client can use it today without writing a single line of integration code.

---

## What it looks like from Claude (or any MCP client)

```
call start_here
→ { status: "initialized", class_count: 10, property_count: 22,
    unseeded_classes: ["Event", "Location", ...],
    schema_first_rule: "Declare properties via add_property before create_entity can store them." }

call create_entity("Person", { name: "Alice", email: "alice@example.com" })
→ { node_id: "4294967296", canonical_label: "Person", created: true }

call create_entity("Person", { name: "Alice", typo_field: "oops" })
→ Error: Unknown property 'typo_field'. Valid: ["name", "email", "phone", "location"]
```

The schema enforces itself. The agent can't write garbage — it gets told exactly what's valid and what isn't.

---

## 2-minute setup (MCP)

**stdio** — drop into Claude Desktop or Claude Code:

```bash
cargo build --release -p sparrowdb-ontology-mcp
```

```json
{
  "mcpServers": {
    "sparrow-ontology": {
      "command": "/path/to/sparrow-ontology-mcp",
      "args": ["--db", "/path/to/your.db"]
    }
  }
}
```

**HTTP** — remote access, Cloudflare tunnel, iOS:

```bash
sparrow-ontology-mcp --db my.db --transport http --port 3456
# sparrow-ontology-mcp listening on http://0.0.0.0:3456
#   POST /mcp   — JSON-RPC endpoint
#   GET  /health — health check
```

---

## MCP Tools

| Tool | What it does |
|------|-------------|
| `start_here` | Init check, property seeding status, schema-first orientation |
| `get_ontology` | Full schema: classes, relations, aliases, declared properties |
| `define_class` | Add a new entity type |
| `define_relation` | Add a relation with domain + range constraints |
| `define_subclass` | Subclass hierarchy — cycle-safe |
| `define_subproperty` | Sub-relation hierarchy — cycle-safe |
| `add_alias` | Register spelling aliases (`"person"` → `Person`, `"org"` → `Organization`) |
| `add_property` | Declare typed, required/optional properties on a class |
| `resolve_name` | Resolve alias to canonical symbol |
| `create_entity` | Write a validated entity node |
| `update_entity` | Update properties on an existing entity |
| `find_entities` | Query by class + optional property filters |
| `create_relationship` | Write a validated, domain/range-checked relationship |
| `explain_symbol` | Full detail on a class or relation: properties, aliases, hierarchy |
| `validate` | Dry-run validation before writing |

---

## Core library (Rust)

```toml
[dependencies]
sparrowdb = { git = "https://github.com/ryaker/SparrowDB" }
sparrowdb-ontology-core = { git = "https://github.com/ryaker/SparrowOntology" }
```

```rust
use sparrowdb::GraphDb;
use sparrowdb_ontology_core::{init, create_entity, create_relationship, find_entities};

let db = GraphDb::open("my.db")?;
init(&db)?;  // seeds 10 classes, 19 relations, 22 properties

// Validated writes — schema checked before any data hits the graph
let alice = create_entity(&db, "Person", &[("name", "Alice"), ("email", "alice@example.com")])?;
let acme  = create_entity(&db, "Organization", &[("name", "Acme Corp")])?;

// Alias resolution built in — agents can write loosely, storage stays clean
let bob = create_entity(&db, "person", &[("name", "Bob")])?;   // "person" → Person ✓

// Domain/range enforcement on relationships
create_relationship(&db, &alice.id, "WORKS_FOR", &acme.id, &[])?;
create_relationship(&db, &acme.id, "WORKS_FOR", &alice.id, &[])?;
// ↑ Error: DomainViolation { relation: "WORKS_FOR", expected: ["Person"], got: "Organization" }

// Subclass expansion — query "Agent", get Person + Organization nodes
let agents = find_entities(&db, "Agent", None, None)?;
```

Error messages are actionable, not cryptic:

```
SoError::UnknownClass {
    name: "Preson",
    suggestion: Some("Did you mean: Person?"),
}

SoError::DomainViolation {
    relation: "WORKS_FOR",
    expected: ["Person"],
    got: "Document",
}

SoError::UnknownSymbol {
    name: "typo_field",
    kind: "property",
    valid: ["name", "email", "phone"],
}
```

### Bulk import with field mapping

```rust
use sparrowdb_ontology_core::{import_records, ImportTemplate};

let template = ImportTemplate {
    class_name: "Person".into(),
    property_map: vec![
        ("full_name".into(), "name".into()),    // source field → ontology field
        ("work_email".into(), "email".into()),
    ],
};

let result = import_records(&db, &template, records)?;
// result.imported == N, result.errors has per-row detail for anything that didn't pass validation
```

---

## Pre-seeded world model

`init()` gives you a usable starting schema in one call. Extend it or replace it entirely.

**Classes (10):** `Person`, `Organization`, `Project`, `Document`, `Event`, `Location`, `Concept`, `Asset`, `Role`, `Claim`

**Relations (19):**

| Relation | Domain → Range |
|----------|----------------|
| `KNOWS` | Person → Person |
| `WORKS_FOR` | Person → Organization |
| `MEMBER_OF` | Person → Organization |
| `LEADS` | Person → Project |
| `LOCATED_IN` | * → Location |
| `PARTICIPATED_IN` | Person → Event |
| `AUTHORED` | Person → Document |
| `OWNS` | Person → Asset |
| `DEPENDS_ON` | Project → Project |
| `CITES` | Document → Document |
| `TAGGED_WITH` | * → Concept |
| `HAS_ROLE` | Person → Role |
| `PRODUCED` | Organization → Asset |
| `OCCURRED_AT` | Event → Location |
| `SUPPORTS` | Claim → Claim |
| `CONTRADICTS` | Claim → Claim |
| `RELATED_TO` | * → * |
| `PART_OF` | * → * |
| `DERIVED_FROM` | * → * |

**Properties (22):** `name`, `description`, `email`, `phone`, `url`, `source`, `confidence`, `start_date`, `end_date`, `created_at`, `updated_at`, and 11 others.

Start blank instead: `--blank` (CLI) or `init_blank(&db)`.

---

## Status

All phases complete. 86 tests, all green. Integration tests run against a real SparrowDB instance — no mocks.

| Phase | What shipped |
|-------|-------------|
| 1 — Core | `sparrowdb-ontology-core`: init, schema ops, entity/relation CRUD, validation, alias resolution, subclass hierarchy |
| 2 — MCP | `sparrowdb-ontology-mcp`: 15 tools, stdio + HTTP transport, E2E tested |
| 3 — CLI | `sparrowdb-ontology-cli`: full `sparrow-ontology` binary with schema/entity/import subcommands |
| 4 — CI/CD | GitHub Actions, release builds, cross-platform binaries |

---

## Why local + embedded

Neo4j Aura costs $65/month minimum. Requires an internet connection. Has a 200MB free tier you'll blow through. Needs a separate process.

SparrowDB is a single Rust library. Sparrow Ontology is a layer on top of it. The whole stack runs in-process, on your laptop, on a Raspberry Pi, behind a Cloudflare tunnel, offline. No subscription. No cloud dependency. No latency you don't control.

For AI agents writing and reading structured knowledge — especially at inference time — that matters.

---

## Architecture

```
MCP client (Claude Desktop / Claude Code / claude.ai)
        │  JSON-RPC 2.0 (stdio or HTTP)
        ▼
sparrow-ontology-mcp
        │  validates schema, resolves aliases, enforces domain/range
        ▼
sparrowdb-ontology-core
        │  reads/writes Cypher queries
        ▼
SparrowDB  (embedded Rust graph engine, zero external deps)
        │  persists to
        ▼
  your.db  (single file)
```

---

## Build

```bash
git clone https://github.com/ryaker/SparrowOntology
cd SparrowOntology
cargo build --workspace
cargo test --workspace
```

Requires Rust 1.75+. SparrowDB is pulled as a git dependency.

---

## Crate layout

```
crates/
├── sparrowdb-ontology-core/    # Library: schema, validation, CRUD, import
├── sparrowdb-ontology-mcp/     # MCP server binary (stdio + HTTP)
└── sparrowdb-ontology-cli/     # sparrow-ontology CLI binary
tests/integration/              # Full roundtrip tests — no mocks
```

---

## Roadmap

- **KMS cutover** — replace the Neo4j Aura ontology layer in production with this stack
- **crates.io publish** — blocked on SparrowDB upstream stabilization
- **Property inheritance** — subclass entities validated against parent-class required properties
- **SPARQL-style query surface** — pattern matching across the ontology without raw Cypher

---

## Tracking

Linear: [Sparrow Ontology project](https://linear.app/sparrowdb/project/sparrow-ontology-d0dd0956d1f0)
Graph engine: [SparrowDB](https://github.com/ryaker/SparrowDB)

---

## License

MIT — see [LICENSE](LICENSE).
