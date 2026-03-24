# Winchester Mystery House Audit — SparrowOntology

**Date:** 2026-03-23
**Auditor:** Claude (Opus 4.6)
**Scope:** All .rs source files across 3 crates, plus LaunchAgent and build artifacts
**Method:** Full source read of every module, import tree trace from both `main.rs` entry points, `cargo check`

---

## Executive Summary

**Verdict: Surprisingly clean.** This is NOT a Winchester project. The codebase has very few dead rooms — nearly everything that is defined is actually wired into a runtime path. There are a handful of findings, but nothing structural. The project compiles with only 3 warnings (2 dead code, 1 unused import).

---

## Compilation Status

```
cargo check → OK (0 errors, 3 warnings)
```

### Warnings:

| # | File | Warning | Severity |
|---|------|---------|----------|
| 1 | `mcp/src/tools/data.rs:101` | `prop_value_to_cypher` is never used | Dead code |
| 2 | `mcp/src/tools/data.rs:113` | `build_props_literal` is never used | Dead code |
| 3 | `mcp/src/main.rs:1` | Unused import `sparrowdb_ontology_mcp::error` | Unused import |
---

## Finding 1: Dead Helper Functions in `data.rs` (Winchester Room)

**Location:** `crates/sparrowdb-ontology-mcp/src/tools/data.rs` lines 101–125

Two functions are defined but never called anywhere:

```rust
fn prop_value_to_cypher(v: &PropertyValue) -> Option<String> { ... }
fn build_props_literal(props: &HashMap<String, PropertyValue>) -> String { ... }
```

**Analysis:** These convert `PropertyValue` maps into inline Cypher `{key: val, ...}` literals. They were likely written for a Cypher-based `CREATE (n:Label {props})` approach to entity creation but were superseded by the `WriteTx::merge_node` approach that all runtime code actually uses. The `props_to_store()` + `WriteTx` path is what `create_entity`, `update_entity`, and `create_relationship` all use.

**Classification:** Classic Winchester room — built, never connected to a hallway.

**Fix:** Delete both functions. Zero callers.

---

## Finding 2: Unused Import in `mcp/main.rs`

**Location:** `crates/sparrowdb-ontology-mcp/src/main.rs` line 1

```rust
use sparrowdb_ontology_mcp::error;
```

**Analysis:** The `main.rs` dispatches to `handle_request()` which calls `tools::handle_tool_call()`. Error handling within `main.rs` itself uses inline `json!()` construction for JSON-RPC errors, never the structured `error::so_error_to_mcp()` helpers. The `error` module IS used — but by `tools/schema.rs` and `tools/data.rs`, not by `main.rs`.

**Fix:** Remove the unused import line.
---

## Finding 3: `#[allow(dead_code)]` Markers in `resolution.rs`

**Location:** `crates/sparrowdb-ontology-core/src/resolution.rs` lines 240–257

Two helper functions are explicitly marked `#[allow(dead_code)]`:

```rust
#[allow(dead_code)]
pub(crate) fn i64_from_value(v: &Value) -> Result<i64, SoError> { ... }

#[allow(dead_code)]
pub(crate) fn bool_from_value(v: &Value) -> bool { ... }
```

**Analysis:** `str_from_value()` (same file, no allow marker) IS used by the `resolve()` function. But `i64_from_value()` and `bool_from_value()` are not called by any runtime path. They exist as utility companions written in anticipation of future use. The `#[allow(dead_code)]` markers were added to silence the compiler rather than removing them.

**Classification:** Intentional stubs, but still dead code.

**Fix:** Either remove them or document what future feature needs them.

---

## Finding 4: Stubbed Full-Graph `validate()` in Core (Bypassed)

**Location:** `crates/sparrowdb-ontology-core/src/validation.rs` lines 379–420

The core library defines these types and a stubbed function:

```rust
pub struct ValidationReport { ... }
pub struct ValidationViolation { ... }
pub struct ValidationWarning { ... }
pub enum ViolationKind { ... }
pub struct ValidationStats { ... }

pub fn validate(_db: &GraphDb) -> Result<ValidationReport, SoError> {
    Ok(ValidationReport { violations: Vec::new(), warnings: Vec::new(), stats: ... })
}
```
The comment says: `TODO SPA-209: Full implementation requires db.labels() and db.relationship_types()`.

**But here's the key finding:** The MCP `validate` tool in `data.rs` does NOT call this stubbed function. Instead, it implements its own full-graph validation directly (~180 lines in `data.rs`), including:
- Checking every `__SO_Relation` has DOMAIN and RANGE edges
- Full label scanning via `CALL db.schema()` with fallback
- Unknown class detection
- Per-class unseeded warnings

**So the MCP validate tool WORKS**, but the core library's `validate()` function is dead. The core types (`ValidationReport`, `ValidationViolation`, `ViolationKind`, `ValidationStats`, `ValidationWarning`) are also dead — nothing imports or uses them.

**Classification:** Winchester room with furniture. The types were defined for a future "proper" implementation that got bypassed by a pragmatic inline implementation at the MCP layer.

**Fix:** Either (a) delete the core `validate()` stub and its types, since the MCP layer has its own working implementation, or (b) refactor the MCP validation logic down into core using these types.

---

## Finding 5: `OntologyConstraint` / `ConstraintKind` — Defined, Never Stored

**Location:** `crates/sparrowdb-ontology-core/src/model.rs`

```rust
pub enum ConstraintKind { Unique, NotNull, Enum(Vec<String>) }
pub struct OntologyConstraint { ... }
```

And in `namespace.rs`:

```rust
pub const CONSTRAINT_LABEL: &str = "__SO_Constraint";
pub const HAS_CONSTRAINT_REL: &str = "__SO_HAS_CONSTRAINT";
```
**Analysis:** These types are defined and the namespace constants exist, but:
- No code ever creates `__SO_Constraint` nodes
- No code ever creates `__SO_HAS_CONSTRAINT` edges
- `OntologyConstraint` is never instantiated
- `ConstraintKind` is never matched on
- The comment on `ConstraintKind::Unique` says "Advisory in v1 — validate() only, not enforced on write"
- `HAS_CONSTRAINT_REL` is imported in `init.rs` but never used in any function body

**Classification:** Spec'd but never built. The namespace constants and types are the architectural scaffolding for a constraint system that was designed but never implemented.

**Fix:** Keep if planned for v2. Mark with `#[allow(dead_code)]` and add a `// v2 planned` comment if intentional. Otherwise delete.

---

## Finding 6: `SOURCE_LABEL_KEY` / `SOURCE_REL_KEY` — Partially Connected

**Location:** `crates/sparrowdb-ontology-core/src/namespace.rs` lines 27–30

```rust
pub const SOURCE_LABEL_KEY: &str = "__so_source_label";
pub const SOURCE_REL_KEY: &str = "__so_source_rel";
```

**Analysis:**
- `SOURCE_LABEL_KEY` / `__so_source_label`: IS used. The `create_entity` MCP tool in `data.rs` injects this when `preserve_source_terms=true && was_alias=true`. And `ALLOWED_SO_KEYS` in `validation.rs` permits it through validation. **Connected.**
- `SOURCE_REL_KEY` / `__so_source_rel`: Is in `ALLOWED_SO_KEYS` (so validation permits it), but no code ever WRITES this key. The `create_relationship` tool does NOT inject `__so_source_rel` even when the relation was resolved from an alias. **Half-connected.**

**Classification:** `SOURCE_REL_KEY` is a Winchester window — you can see through it (validation allows it) but you can't reach it (no code writes it).

**Fix:** Either add `__so_source_rel` injection to `create_relationship` (matching the `create_entity` pattern), or document it as "user-settable only."
---

## Finding 7: `StarterKind::Blank` Maps to WorldModel (Logic Bug)

**Location:** `crates/sparrowdb-ontology-core/src/init.rs` lines 93–95

```rust
StarterKind::WorldModel | StarterKind::Blank => (
    canonical_world_model(),
    canonical_world_model_relations(),
    canonical_world_model_properties(),
),
```

**Analysis:** `StarterKind::Blank` is supposed to be "no classes or relations seeded" according to the `start_here` tool description: *"Empty schema — no classes or relations seeded. Use define_class and define_relation to build from scratch."* But it maps to the same arm as `WorldModel`, meaning it seeds the full 10-class, 19-relation, 22-property world model. This is a logic bug, not dead code.

**Classification:** Not a Winchester room — it's a door that opens to the wrong room.

**Fix:** `StarterKind::Blank` should return `(vec![], vec![], vec![])`.

---

## Finding 8: `expand_subproperties` — Exported but Never Called

**Location:** `crates/sparrowdb-ontology-core/src/hierarchy.rs`

The function `expand_subproperties()` is public and does BFS expansion of sub-relations via `__SO_SUBPROPERTY_OF`. However, searching all callers:

- `expand_subclasses` is used by `data.rs` (explain_symbol, find_entities) and re-exported from `validation.rs`. **Connected.**
- `expand_subproperties` is NOT called by any MCP tool, CLI command, or other function. **Dead.**

The `define_subproperty` MCP tool creates subproperty edges, and `explain_relation` reads them via direct Cypher. But nobody calls `expand_subproperties()` to get the transitive closure.

**Classification:** Winchester staircase — goes somewhere but nothing at the top.

**Fix:** Either wire it into `explain_relation` (to show transitive sub-relations) or mark it as utility for future use.
---

## Finding 9: `init` Tool Referenced in `start_here` but Not Registered as MCP Tool

**Location:** `crates/sparrowdb-ontology-mcp/src/main.rs` `tool_list()` function

The `start_here` response says: *"Call the init tool with a starter parameter to bootstrap the ontology."*

But looking at `tool_list()` — there is no `init` tool listed. And in `tools/mod.rs`, the `handle_tool_call` dispatcher has no arm for `"init"`. The `init()` function exists in the core crate and the CLI has `Commands::Init`, but the MCP server has no way to call it.

**Classification:** Ghost tool — the documentation references it, but the door doesn't exist.

**Impact:** An LLM agent using the MCP server would be told to "call init" but would get `Method not found`. The workaround is to use the CLI or to just use the already-initialized database (which the LaunchAgent does).

**Fix:** Either register an `init` MCP tool, or change the `start_here` message to say "Use the CLI: `sparrow-ontology init --db <path>`".

---

## Finding 10: `find_entities` Accepts `where` but Schema Says `filters`

**Location:** `crates/sparrowdb-ontology-mcp/src/main.rs` `tool_list()` vs `crates/sparrowdb-ontology-mcp/src/tools/data.rs`

The `tool_list()` schema advertises:
```json
"filters": {"type": "object", "description": "Optional property key-value filters"}
```

But the actual implementation in `find_entities()` reads:
```rust
if let Some(obj) = args["where"].as_object() { ... }
```

It looks for `"where"`, not `"filters"`. An agent following the schema would pass `filters` and get no filtering.

**Classification:** Schema/implementation mismatch — the map says one thing, the room is somewhere else.

**Fix:** Either change the schema to `"where"` or change the implementation to read `"filters"` (or accept both with fallback like `create_entity` does for `class_name`/`label`).
---

## Anti-Findings: Things That ARE Properly Connected

For completeness, here's what's NOT Winchester:

| Feature | Defined | Registered | Called | Verdict |
|---------|---------|------------|--------|---------|
| `start_here` | core + MCP schema | tool_list ✓ | dispatch ✓ | **Live** |
| `get_ontology` | MCP schema | tool_list ✓ | dispatch ✓ | **Live** |
| `define_class` | MCP schema | tool_list ✓ | dispatch ✓ | **Live** |
| `define_relation` | MCP schema | tool_list ✓ | dispatch ✓ | **Live** |
| `add_alias` | core + MCP | tool_list ✓ | dispatch ✓ | **Live** |
| `add_property` | core + MCP | tool_list ✓ | dispatch ✓ | **Live** |
| `define_subclass` | core + MCP | tool_list ✓ | dispatch ✓ | **Live** |
| `define_subproperty` | MCP only | tool_list ✓ | dispatch ✓ | **Live** |
| `resolve_name` | core + MCP | tool_list ✓ | dispatch ✓ | **Live** |
| `create_entity` | MCP + validation | tool_list ✓ | dispatch ✓ | **Live** |
| `create_relationship` | MCP + validation | tool_list ✓ | dispatch ✓ | **Live** |
| `update_entity` | MCP + validation | tool_list ✓ | dispatch ✓ | **Live** |
| `find_entities` | MCP + subclass expansion | tool_list ✓ | dispatch ✓ | **Live** |
| `explain_symbol` | MCP (class + relation) | tool_list ✓ | dispatch ✓ | **Live** |
| `validate` | MCP (full impl) | tool_list ✓ | dispatch ✓ | **Live** |
| `health` | MCP + HTTP route | tool_list ✓ | dispatch ✓ | **Live** |
| `stats` | MCP + HTTP route | tool_list ✓ | dispatch ✓ | **Live** |
| CSV/JSON import | core + CLI | CLI wired ✓ | cmd_import ✓ | **Live** |
| All 15 CLI commands | CLI main.rs | clap subcommands ✓ | `run()` dispatch ✓ | **Live** |
| Fuzzy matching | resolution.rs | Used by resolve() | On every miss ✓ | **Live** |
| Cycle detection | hierarchy.rs | define_subclass, define_subproperty | Both paths ✓ | **Live** |
| Property inheritance | validation.rs | validate_entity | On every create/update ✓ | **Live** |
| Subclass-aware domain/range | validation.rs | validate_relationship | On every edge creation ✓ | **Live** |
| All 4 starter templates | model.rs | init() | CLI init command ✓ | **Live** |
---

## TODO Tracker (Open SPA Tickets)

| Ticket | Location | Description | Blocked On |
|--------|----------|-------------|------------|
| SPA-208 | namespace.rs, init.rs | Replace convention-based `__SO_` protection with `reserve_label_prefix()` | SparrowDB upstream |
| SPA-208 | init.rs | `force=true` should wipe before re-seed (needs `delete_edge` + `delete_node`) | SparrowDB upstream |
| SPA-209 | validation.rs, resolution.rs | `db.labels()`, `db.begin_read()?.query()` APIs for proper graph scan | SparrowDB upstream |
| SPA-218 | resolution.rs | Parameterized Cypher (currently using string interpolation + escaping) | SparrowDB upstream |

---

## Summary of Findings

| # | Finding | Severity | Type |
|---|---------|----------|------|
| 1 | `prop_value_to_cypher` / `build_props_literal` dead code | Low | Dead code |
| 2 | Unused `error` import in `main.rs` | Trivial | Unused import |
| 3 | `i64_from_value` / `bool_from_value` explicitly `#[allow(dead_code)]` | Low | Dead code |
| 4 | Core `validate()` stubbed; MCP has its own working impl | Medium | Bypassed module |
| 5 | `OntologyConstraint` / `ConstraintKind` / namespace constants never used | Medium | Spec'd, never built |
| 6 | `__so_source_rel` permitted in validation but never written | Low | Half-connected |
| 7 | `StarterKind::Blank` seeds WorldModel instead of empty | **High** | Logic bug |
| 8 | `expand_subproperties` exported but never called | Low | Dead code |
| 9 | `init` tool referenced in `start_here` but not registered as MCP tool | **High** | Ghost tool |
| 10 | `find_entities` reads `where` but schema advertises `filters` | **High** | Schema mismatch |

**Total lines of dead code:** ~80 lines (out of ~4,200 total .rs lines, excluding tests)
**Dead code percentage:** ~1.9%
**Logic bugs found:** 2 (Blank starter, filters/where mismatch)
**Ghost features:** 1 (init tool in MCP)

---

*Generated by Winchester Audit protocol. No code was modified.*