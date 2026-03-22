# Sparrow Ontology

> A semantic layer for SparrowDB: typed entities, alias resolution, domain/range validation, and a pre-seeded world model — exposed as an MCP server any LLM client can use today.

[![CI](https://github.com/ryaker/SparrowOntology/actions/workflows/ci.yml/badge.svg)](https://github.com/ryaker/SparrowOntology/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

---

## What It Is

SparrowDB stores graphs. Sparrow Ontology gives those graphs a schema.

Without an ontology, any label and any property can go anywhere. That's fine for exploration. It's a problem when a language model is writing nodes, or when you need to merge data from multiple sources and actually trust what you get back.

Sparrow Ontology sits between your application (or an MCP client) and SparrowDB. It enforces a type system: every entity has a class, every relationship has domain and range constraints, every write goes through validation before it touches the graph. Aliases let you write `"person"` and get `Person`. Subclass hierarchies let you query `Agent` and get back both `Person` and `Organization`.

The built-in world model seeds 10 classes, 19 relation types, and 22 properties on init. You can extend it or replace it entirely.

**First production target:** the KMS adapter (SPA-108) — replacing the Neo4j Aura ontology layer with a local, zero-latency, zero-subscription equivalent.

---

## Status — All Phases Complete

| Phase | Crate / Feature | Status |
|-------|----------------|--------|
| 1 | `sparrowdb-ontology-core` — init, schema ops, entity/relation CRUD, validation, alias resolution, subclass hierarchy | ✅ Complete |
| 2 | `sparrowdb-ontology-mcp` — 15 MCP tools, stdio + HTTP transport, E2E tested via Claude Desktop | ✅ Complete |
| 3 | `sparrowdb-ontology-cli` — `sparrow-ontology` binary, all schema/entity/import subcommands | ✅ Complete |
| 4 | CI/CD, GitHub Releases, cross-platform binaries, MCP config docs | ✅ Complete |

**86 tests, all green.** Integration tests cover the full path from MCP JSON-RPC call through SparrowDB and back — not just unit logic.

Recent additions:
- `add_property` — declare typed, required properties on classes (SPA-229)
- `import_records` — template-driven bulk entity import with per-row error reporting (SPA-230)
- HTTP transport — `--transport http --port 3456` for remote access and Cloudflare tunnel (SPA-231)
- E2E test suite against live MCP server via Claude Desktop (SPA-228)

---

## MCP Server — Connect in 2 minutes

`sparrow-ontology-mcp` speaks JSON-RPC 2.0. Drop it into Claude Desktop, Claude Code, or any MCP client.

**stdio** (Claude Desktop / Claude Code):

```bash
# Download from GitHub Releases, or build from source:
cargo build --release -p sparrowdb-ontology-mcp

# Add to Claude Desktop config at ~/Library/Application Support/Claude/claude_desktop_config.json:
```

```json
{
  "mcpServers": {
    "sparrow-ontology": {
      "command": "/usr/local/bin/sparrow-ontology-mcp",
      "args": ["--db", "/path/to/your.db"]
    }
  }
}
```

**HTTP** (remote access, Cloudflare tunnel, iOS):

```bash
sparrow-ontology-mcp --db my.db --transport http --port 3456
# → sparrow-ontology-mcp listening on http://0.0.0.0:3456
#   POST /mcp   — JSON-RPC endpoint
#   GET  /health — health check
```

### Tools

| Tool | What it does |
|------|-------------|
| `start_here` | Check init state, get orientation on next steps |
| `get_ontology` | Full schema: classes, relations, aliases, properties |
| `define_class` | Add a new entity type |
| `define_relation` | Add a relation with domain + range constraints |
| `define_subclass` | Create subclass hierarchy (cycle-safe) |
| `define_subproperty` | Create sub-relation hierarchy (cycle-safe) |
| `add_alias` | Register a spelling alias for a class or relation |
| `add_property` | Declare a typed property on a class (required or optional) |
| `resolve_name` | Resolve an alias to its canonical form |
| `create_entity` | Write a validated entity node |
| `update_entity` | Update properties on an existing entity |
| `find_entities` | Query by class + optional property filters |
| `create_relationship` | Write a validated, domain/range-checked relationship |
| `explain_symbol` | Full detail on a class or relation: properties, aliases, hierarchy |
| `validate` | Check an entity payload against the schema before writing |

---

## Core Library

```toml
[dependencies]
sparrowdb = { git = "https://github.com/ryaker/SparrowDB" }
sparrowdb-ontology-core = { git = "https://github.com/ryaker/SparrowOntology" }
```

```rust
use sparrowdb::GraphDb;
use sparrowdb_ontology_core::{init, create_entity, create_relationship, find_entities};

let db = GraphDb::open("my.db")?;
init(&db)?; // seeds world model: 10 classes, 19 relations, 22 properties

// Create typed entities — validation runs before the write
let alice = create_entity(&db, "Person", &[("name", "Alice"), ("email", "alice@example.com")])?;
let acme  = create_entity(&db, "Organization", &[("name", "Acme Corp")])?;

// Alias resolution: "person" → Person, "org" → Organization
let bob = create_entity(&db, "person", &[("name", "Bob")])?;

// Domain/range validated relationship
create_relationship(&db, &alice.id, "WORKS_FOR", &acme.id, &[])?;

// Subclass expansion: "Agent" returns Person + Organization nodes
let agents = find_entities(&db, "Agent", None, None)?;
```

Error messages tell you what went wrong and what's valid:

```
SoError::DomainViolation {
    relation: "WORKS_FOR",
    expected: ["Person"],
    got: "Document",
}

SoError::UnknownClass {
    name: "Preson",
    suggestion: Some("Did you mean: Person?"),
}
```

### Bulk Import

```rust
use sparrowdb_ontology_core::{import_records, ImportTemplate};

let template = ImportTemplate {
    class_name: "Person".into(),
    property_map: vec![
        ("full_name".into(), "name".into()),
        ("work_email".into(), "email".into()),
    ],
};

let result = import_records(&db, &template, records)?;
// result.imported == N, result.errors contains per-row failures
```

---

## World Model

`init()` seeds the following. All labels use the reserved `__SO_` prefix in SparrowDB; your application always sees plain names.

**Classes (10):** Person, Organization, Project, Document, Event, Location, Concept, Asset, Role, Claim

**Relations (19):**

| Relation | Domain | Range |
|----------|--------|-------|
| KNOWS | Person | Person |
| WORKS_FOR | Person | Organization |
| MEMBER_OF | Person | Organization |
| LEADS | Person | Project |
| LOCATED_IN | * | Location |
| PARTICIPATED_IN | Person | Event |
| AUTHORED | Person | Document |
| RELATED_TO | * | * |
| OWNS | Person | Asset |
| DEPENDS_ON | Project | Project |
| CITES | Document | Document |
| TAGGED_WITH | * | Concept |
| HAS_ROLE | Person | Role |
| PRODUCED | Organization | Asset |
| OCCURRED_AT | Event | Location |
| PART_OF | * | * |
| DERIVED_FROM | * | * |
| SUPPORTS | Claim | Claim |
| CONTRADICTS | Claim | Claim |

**Properties (22):** name, description, created_at, updated_at, source, confidence, url, email, phone, start_date, end_date, and 11 others.

Start with an empty schema instead: `--blank` (CLI) or call `init_blank(&db)`.

---

## Crate Layout

```
SparrowOntology/
├── crates/
│   ├── sparrowdb-ontology-core/   # Library: init, schema, CRUD, validation, import
│   │   └── src/
│   │       ├── init.rs            # World model seeding, add_property
│   │       ├── model.rs           # Class, Relation, Property types
│   │       ├── validation.rs      # Entity + relationship validation
│   │       ├── resolution.rs      # Alias resolution
│   │       ├── hierarchy.rs       # Subclass/subproperty traversal
│   │       ├── import.rs          # Template-driven bulk import
│   │       └── namespace.rs       # __SO_ prefix management
│   ├── sparrowdb-ontology-mcp/    # MCP server binary, 15 tools, stdio + HTTP
│   └── sparrowdb-ontology-cli/    # sparrow-ontology binary
└── tests/integration/             # Integration tests (full roundtrip, not mocks)
```

---

## Build and Test

```bash
git clone https://github.com/ryaker/SparrowOntology
cd SparrowOntology
cargo build --workspace
cargo test --workspace   # 86 tests, all green
```

Requires Rust 1.75+ and a local SparrowDB (see `Cargo.toml` path patch).

### Testing philosophy

All integration tests run against a real SparrowDB instance — no mocks. Unit tests prove compartmentalized logic. Integration tests prove the system. Both matter; only one is sufficient to ship.

---

## Roadmap

- **SPA-108** — KMS adapter: swap Neo4j Aura for Sparrow Ontology (zero-subscription, local-first)
- **SPA-226** — crates.io publish (blocked on SparrowDB upstream SPA-210)
- **Property inheritance in validation** — subclass entities validated against parent-class required properties

---

## Spec and Tracking

Linear project: [Sparrow Ontology](https://linear.app/sparrowdb/project/sparrow-ontology-d0dd0956d1f0)

SparrowDB (the graph engine underneath): [github.com/ryaker/SparrowDB](https://github.com/ryaker/SparrowDB)

---

## License

MIT — see [LICENSE](LICENSE).
