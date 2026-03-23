# Sparrow Ontology

**Schema-enforced graph memory for AI agents — embedded, local, zero infrastructure.**

[![CI](https://github.com/ryaker/SparrowOntology/actions/workflows/ci.yml/badge.svg)](https://github.com/ryaker/SparrowOntology/actions/workflows/ci.yml)
[![Crates.io (core)](https://img.shields.io/crates/v/sparrowdb-ontology-core.svg)](https://crates.io/crates/sparrowdb-ontology-core)
[![Crates.io (mcp)](https://img.shields.io/crates/v/sparrowdb-ontology-mcp.svg)](https://crates.io/crates/sparrowdb-ontology-mcp)
[![Crates.io (cli)](https://img.shields.io/crates/v/sparrowdb-ontology-cli.svg)](https://crates.io/crates/sparrowdb-ontology-cli)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

---

AI agents are terrible at writing graphs consistently. Every session they spell `Person` differently, store email as `email` or `e_mail` or `contact`, and invent relationship names on the fly. You end up with a graph full of data that's useless for retrieval.

Sparrow Ontology puts a typed schema between the agent and the graph. Every write is validated before it touches storage. Spelling aliases collapse to canonical names. Domain/range constraints catch bad relationships at write time. And errors tell the agent exactly what to fix — no debugging required.

It runs in-process on top of [SparrowDB](https://github.com/ryaker/SparrowDB), a pure-Rust embedded graph engine. No Neo4j, no cloud, no subscription.

---

## What it looks like

The server speaks MCP, so Claude Desktop, Claude Code, or any MCP client can use it without writing integration code:

```
→ start_here
{ initialized: true, class_count: 10, property_count: 22,
  unseeded_classes: ["Event", "Location", "Project", ...] }

→ create_entity("Person", { name: "Alice", email: "alice@example.com" })
{ node_id: "4294967296", canonical_label: "Person", created: true }

→ create_entity("Person", { name: "Alice", typo_field: "oops" })
Error: Unknown property 'typo_field'. Valid: ["name", "email", "phone", "location"].
Call add_property(owner='Person', name='typo_field') to declare it first.

→ create_entity("person", { name: "Bob" })
{ node_id: "4294967297", canonical_label: "Person", created: true }
# lowercase "person" auto-resolved — no alias registration needed for exact case matches
```

The schema enforces itself. The agent can't write garbage because it gets told what's valid and what to do about it.

---

## Setup (2 minutes)

**stdio** — Claude Desktop or Claude Code:

```bash
cargo install sparrowdb-ontology-mcp
```

```json
{
  "mcpServers": {
    "sparrow-ontology": {
      "command": "sparrow-ontology-mcp",
      "args": ["--db", "/path/to/your.db"]
    }
  }
}
```

**Build from source:**

```bash
git clone https://github.com/ryaker/SparrowOntology
cd SparrowOntology
cargo build --release -p sparrowdb-ontology-mcp
```

**HTTP** — remote access, Cloudflare tunnel, mobile:

```bash
sparrow-ontology-mcp --db my.db --transport http --port 3456
# POST /mcp     — JSON-RPC endpoint
# GET  /health  — operational health check
# GET  /ontology/stats — schema analytics
```

---

## Why embedded matters

Neo4j Aura starts at $65/month. Requires a network connection. Has a 200MB free tier you'll blow through in a week. Needs a separate process to manage.

SparrowDB links directly into your binary. Sparrow Ontology sits on top of it. The whole stack runs in-process — on your laptop, a Raspberry Pi, behind a Cloudflare tunnel, completely offline. One `.db` file. No daemon. No bill.

For AI agents reading and writing structured knowledge at inference time, local-first with zero round-trip latency isn't a nice-to-have. It's the difference between something you can actually ship and something you're still provisioning.

---

## MCP Tools (17 total)

| Tool | What it does |
|------|-------------|
| `start_here` | Schema orientation: class counts, unseeded classes, schema-first workflow. Accepts optional `template` param. |
| `health` | Operational ping — returns `{"status": "ok"}`. Call before any write session. |
| `stats` | Schema analytics: class/relation/property counts, entity counts per class, totals. |
| `get_ontology` | Full schema dump: classes, relations, aliases, properties |
| `define_class` | Add a new entity type |
| `define_relation` | Add a typed relation with domain + range constraints |
| `define_subclass` | Subclass hierarchy — cycle detection built in, inherits required properties |
| `define_subproperty` | Property hierarchy — subproperty inherits from parent property |
| `add_alias` | Register spelling variants (`"org"` → `Organization`) |
| `add_property` | Declare typed properties on a class (required or optional) |
| `create_entity` | Write a validated entity — schema checked before storage |
| `update_entity` | Update properties on an existing entity |
| `find_entities` | Query by class + optional property filters |
| `create_relationship` | Write a domain/range-validated relationship edge |
| `explain_symbol` | Full detail on a class or relation: properties, aliases, hierarchy |
| `validate` | Dry-run validation without writing |
| `resolve_name` | Resolve an alias to its canonical symbol |

---

## Starter Templates

`start_here` accepts an optional `template` param to seed a domain schema in one call:

| Template | Classes | Use when |
|----------|---------|----------|
| `WorldModel` | 10 general-purpose | Default. Covers most agentic tasks. |
| `PersonalKnowledge` | Person, Concept, Event, Location, Document | Personal memory, notes, contact graphs |
| `ProfessionalNetwork` | Person, Organization, Role, Project, Event | Team ontologies, org charts, project tracking |
| `ResearchNotes` | Concept, Document, Claim, Person, Asset | Research, citations, evidence chains |

```
→ start_here({ "template": "ProfessionalNetwork" })
{ initialized: true, class_count: 5, template: "ProfessionalNetwork", ... }
```

---

## Rust library

```toml
[dependencies]
sparrowdb-ontology-core = "0.1"
```

```rust
use sparrowdb::GraphDb;
use sparrowdb_ontology_core::{init, ValidationContext};

let db = GraphDb::open("my.db")?;
init(&db, None, false)?;  // seeds 10 classes, 19 relations, 22 properties

// Schema-validated writes
let mut ctx = ValidationContext::new(&db);
ctx.validate_entity("Person", &props, true)?;
// → Err if unknown property, type mismatch, or required field missing

// Alias resolution: "person" → Person, "org" → Organization
let resolved = resolve(&db, "person", AliasKind::Class)?;
// → ResolvedSymbol { canonical_name: "Person", was_alias: true }
```

Errors are actionable, not cryptic:

```
SoError::UnknownSymbol {
    name: "typo_field",
    kind: "property",
    valid: ["name", "email", "phone"],
    suggestion: Some("Did you mean 'email'? Call add_alias(...) to register permanently."),
}

SoError::DomainViolation {
    relation: "WORKS_FOR",
    expected: "Person",
    actual: "Document",
    // message: "Relation 'WORKS_FOR' requires the source entity to be of class 'Person',
    //           but got 'Document'. Call explain_symbol('WORKS_FOR') to see full constraints."
}
```

### Bulk import

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
// result.imported == N
// result.errors — per-row detail for validation failures
```

---

## Pre-seeded schema

`init()` gives you a working schema in one call. Extend it or replace it entirely with `init_blank()`.

**10 classes:** `Person`, `Organization`, `Project`, `Document`, `Event`, `Location`, `Concept`, `Asset`, `Role`, `Claim`

**19 relations** with domain/range constraints: `KNOWS`, `WORKS_FOR`, `MEMBER_OF`, `LEADS`, `AUTHORED`, `OWNS`, `DEPENDS_ON`, `CITES`, `TAGGED_WITH`, `HAS_ROLE`, `PRODUCED`, `PARTICIPATED_IN`, `LOCATED_IN`, `OCCURRED_AT`, `SUPPORTS`, `CONTRADICTS`, `RELATED_TO`, `PART_OF`, `DERIVED_FROM`

**22 properties:** `name`, `description`, `email`, `phone`, `url`, `source`, `confidence`, `start_date`, `end_date`, `created_at`, `updated_at`, and more.

---

## Architecture

```
MCP client (Claude Desktop / Claude Code / any MCP host)
        │  JSON-RPC 2.0 over stdio or HTTP
        ▼
sparrow-ontology-mcp
        │  validates schema · resolves aliases · enforces domain/range
        ▼
sparrowdb-ontology-core
        │  Cypher queries
        ▼
SparrowDB  (embedded Rust graph engine · zero external deps)
        │
        ▼
  your.db
```

---

## Build

```bash
git clone https://github.com/ryaker/SparrowOntology
cd SparrowOntology
cargo build --workspace
cargo test --workspace       # 117 tests, all integration, no mocks
```

Requires Rust 1.75+.

```
crates/
├── sparrowdb-ontology-core/   # Schema, validation, CRUD, import
├── sparrowdb-ontology-mcp/    # MCP server binary (stdio + HTTP)
└── sparrowdb-ontology-cli/    # sparrow-ontology CLI binary
tests/integration/             # Full roundtrip tests against real SparrowDB
```

---

## Docs

- [Agent World Model](docs/agent-world-model.md) — multi-agent coordination, schema as contract
- [Personal Ontology](docs/personal-ontology.md) — durable AI memory for one person
- [Team Ontology](docs/team-ontology.md) — shared schema for humans + agents
- [MCP Reference](docs/mcp-reference.md) — all 17 tools with parameters and examples
- [Schema Reference](docs/schema-reference.md) — WorldModel classes, relations, properties
- [Research Ontology](docs/research-ontology.md) — claims, evidence, provenance

---

## License

MIT — see [LICENSE](LICENSE).
