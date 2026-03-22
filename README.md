# Sparrow Ontology

**Ontology-aware semantic layer for SparrowDB**

[![CI](https://github.com/ryaker/SparrowOntology/actions/workflows/ci.yml/badge.svg)](https://github.com/ryaker/SparrowOntology/actions/workflows/ci.yml)

Sparrow Ontology gives SparrowDB a semantic layer: type-safe entity creation, alias resolution, domain/range validation, and a pre-seeded world model that scales from a single person to a company of thousands.

---

## Quick Start — Download and first ontology-aware write in 5 steps

**1. Download the `sparrow-ontology` binary from GitHub Releases**

Go to the [Releases page](https://github.com/ryaker/SparrowOntology/releases) and download the archive for your platform:

| Platform | Archive |
|----------|---------|
| Linux x86_64 | `sparrow-ontology-x86_64-unknown-linux-gnu.tar.gz` |
| Linux arm64 | `sparrow-ontology-aarch64-unknown-linux-gnu.tar.gz` |
| macOS Apple Silicon | `sparrow-ontology-aarch64-apple-darwin.tar.gz` |
| macOS Intel | `sparrow-ontology-x86_64-apple-darwin.tar.gz` |
| Windows x86_64 | `sparrow-ontology-x86_64-pc-windows-msvc.zip` |

Extract the archive and place `sparrow-ontology` (or `sparrow-ontology.exe`) on your `PATH`.

**2. Initialize the ontology database**

```sh
sparrow-ontology init --db my.db
```

This seeds the database with the built-in world model (Person, Organization, and core relations). Pass `--blank` to start with an empty schema.

**3. Create a Person entity**

```sh
sparrow-ontology create-entity Person --db my.db --props '{"name":"Alice"}'
```

Output includes the assigned `node_id`, e.g. `node_id=abc123`.

**4. Create an Organization entity**

```sh
sparrow-ontology create-entity Organization --db my.db --props '{"name":"Acme Corp"}'
```

Output includes `node_id=def456`.

**5. Create a typed relationship**

```sh
sparrow-ontology create-relationship --db my.db --from abc123 --type WORKS_FOR --to def456
```

The relation is validated against the ontology domain/range before being written.

---

## MCP Server

`sparrow-ontology-mcp` implements the [Model Context Protocol](https://modelcontextprotocol.io/) over stdin/stdout JSON-RPC 2.0. Connect it to any MCP-compatible client (e.g. Claude Desktop).

### Claude Desktop config snippet

Add to `~/Library/Application Support/Claude/claude_desktop_config.json` (macOS) or the equivalent on your platform:

```json
{
  "mcpServers": {
    "sparrow-ontology": {
      "command": "/usr/local/bin/sparrow-ontology-mcp",
      "args": ["--db", "/path/to/your/database"]
    }
  }
}
```

Replace `/usr/local/bin/sparrow-ontology-mcp` with the actual path to the binary and `/path/to/your/database` with the path to your SparrowDB database directory.

---

## Install from source

Requires Rust 1.75+.

```sh
# Install the CLI
cargo install --git https://github.com/ryaker/SparrowOntology sparrowdb-ontology-cli

# Install the MCP server
cargo install --git https://github.com/ryaker/SparrowOntology sparrowdb-ontology-mcp
```

Or clone and build locally:

```sh
git clone https://github.com/ryaker/SparrowOntology
cd SparrowOntology
cargo build --release -p sparrowdb-ontology-cli -p sparrowdb-ontology-mcp
# Binaries at: target/release/sparrow-ontology and target/release/sparrow-ontology-mcp
```

---

## CLI reference

```
sparrow-ontology --help
sparrow-ontology --version

sparrow-ontology init            Initialize the ontology in a database
sparrow-ontology show            Show the current ontology
sparrow-ontology define-class    Define a new ontology class
sparrow-ontology define-relation Define a new relation between classes
sparrow-ontology add-alias       Register an alias for a class or relation
sparrow-ontology add-subclass    Declare a subclass relationship
sparrow-ontology validate        Validate the graph
sparrow-ontology create-entity   Create a typed entity node
sparrow-ontology create-relationship  Create a typed relationship edge
sparrow-ontology explain         Explain a symbol in detail
sparrow-ontology stats           Show ontology and graph statistics
```

---

## Crate layout

| Crate | Description |
|-------|-------------|
| `sparrowdb-ontology-core` | Core library: ontology init, schema ops, validation |
| `sparrowdb-ontology-mcp` | MCP server binary + tool handler library |
| `sparrowdb-ontology-cli` | CLI binary (`sparrow-ontology`) |

---

## Documentation / Spec

Implementation spec: [Linear project](https://linear.app/sparrowdb/project/sparrow-ontology-d0dd0956d1f0)

---

## License

MIT
