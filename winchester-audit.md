# Winchester Mystery House Audit: SparrowOntology

**Audited**: 2026-03-22
**Location**: `/Users/ryaker/Dev/SparrowOntology`
**Language**: Rust (workspace)
**Verdict**: The entire project is a Winchester Mystery House — scaffolding with no rooms.

---

## Executive Summary

SparrowOntology is a Rust workspace that describes itself as "an ontology semantic layer on top of SparrowDB" providing "semantic alias normalization, write-time validation, hierarchy expansion, and a guided world-model bootstrap." The README claims Phase 1 implements the ontology metadata model, alias resolution, validation engine, hierarchy expansion, and world model bootstrap across 24 acceptance criteria.

**None of this exists.** There is zero source code. Zero tests. Zero commits.

The project is a fully articulated skeleton — Cargo workspace, crate structure, dependency declarations, toolchain config, README with build instructions — wrapped around absolute emptiness.

---

## What Exists (The Scaffolding)

### Files present (5 total, excluding .git)

| File | Purpose | Status |
|------|---------|--------|
| `Cargo.toml` | Workspace root — declares `crates/sparrowdb-ontology-core` | Functional config, nothing to build |
| `crates/sparrowdb-ontology-core/Cargo.toml` | Crate manifest — declares 6 dependencies | Parseable, but no `src/lib.rs` or `src/main.rs` to compile |
| `README.md` | Documents Phase 1 features and 24 acceptance criteria | Describes a system that doesn't exist |
| `rust-toolchain.toml` | Pins to stable Rust | Valid but irrelevant — nothing to compile |
| `LICENSE` | MIT license | The only fully delivered artifact |

### Directories present (empty)

| Directory | Expected Contents | Actual Contents |
|-----------|-------------------|-----------------|
| `crates/sparrowdb-ontology-core/src/` | `lib.rs`, module files for ontology core | **Empty** |
| `tests/integration/` | Integration test files | **Empty** |

### Dependencies declared but never used

The crate's `Cargo.toml` declares these dependencies, none of which are imported anywhere because there is no source code:

- `sparrowdb` (git dep from `github.com/ryaker/SparrowDB`, branch main) — the parent DB this ontology layer sits on
- `uuid` v1 (with v4 + serde features)
- `serde` v1 (with derive)
- `serde_json` v1
- `thiserror` v1
- `chrono` v1
- `tempfile` v3 (dev-dependency)

### Git state

- Branch: `main`
- Commits: **0** (no commits have ever been made)
- All files are untracked

---

## Audit Findings by Category

### 1. Dead Code
**N/A** — There is no code, dead or alive. The project is pre-code.

### 2. Disconnected Modules
**The entire project is disconnected from itself.** The workspace declares a crate member (`sparrowdb-ontology-core`) whose `src/` directory is empty. The crate cannot compile — Rust requires at minimum a `src/lib.rs` or `src/main.rs`. Running `cargo build` would fail immediately.

### 3. Stubs and Partially Finished Features
**Everything is a stub.** Specifically:

- **README promises 4 capabilities**: alias normalization, write-time validation, hierarchy expansion, world-model bootstrap. Zero lines of implementation exist for any of them.
- **README references "24 acceptance criteria"** that must pass before Phase 2. No tests exist — not in `tests/integration/`, not as unit tests, nowhere. The criteria are undefined in any file.
- **Phase 2 (MCP server)** is mentioned as the next step. Phase 1 hasn't started.
- **Crate `src/` directory exists** but contains no files — the directory was created, the files never were.

### 4. Config Parsed but Not Enforced
**Cargo.toml is the config, and it configures nothing real.** The workspace `Cargo.toml` and crate `Cargo.toml` are syntactically valid and would be parsed by Cargo, but since there's no source code, every declared dependency is dead weight. The `rust-toolchain.toml` pins to stable Rust for a project that has nothing to compile.

### 5. Tech Debt
**No tech debt in the traditional sense** — you need code to accumulate debt. However, there is *architectural debt*:

- **The README is a liability.** It describes a system that doesn't exist. If someone reads it, they'll believe Phase 1 is implemented and wonder why it doesn't compile. This is worse than no README — it's misinformation.
- **Dependency pre-declarations.** The crate declares 6 runtime dependencies and 1 dev-dependency based on anticipated needs. These will likely need revision once actual implementation begins — dependency versions will be stale, feature flags may be wrong, and `sparrowdb` git dependency may have diverged.

---

## The Winchester Mystery House Pattern

The classic Winchester pattern is "beautifully built rooms that lead nowhere." SparrowOntology is a more extreme variant: **beautifully built hallways with no rooms at all.**

| Winchester Element | SparrowOntology Manifestation |
|-------------------|-------------------------------|
| Doors to nowhere | `crates/sparrowdb-ontology-core/src/` — a source directory with no source |
| Stairs to the ceiling | README describing Phase 1 completion criteria for code that was never written |
| Windows in interior walls | `tests/integration/` — a test directory with no tests for code that doesn't exist |
| Unused plumbing | 7 Cargo dependencies declared, 0 imported |
| Rooms built over rooms | Phase 2 (MCP server) planned on top of Phase 1 that hasn't started |

---

## Recommendations

1. **Decide if this project is alive or dead.** If alive, the next step is writing `src/lib.rs` with the core ontology types. If dead, delete the repo.
2. **If alive, strip the README** down to a one-liner and rebuild it as features are actually implemented. The current README is a roadmap masquerading as documentation.
3. **Re-evaluate dependencies** at implementation time. The `sparrowdb` git dependency pointing to `main` is a moving target — pin to a specific commit or tag when you start building.
4. **Define the 24 acceptance criteria** in an actual file (e.g., `ACCEPTANCE_CRITERIA.md` or as `#[test]` stubs) so they're trackable.
5. **Consider whether SparrowOntology should be a crate inside the SparrowDB repo** rather than a separate repository, given that it has zero independent functionality.

---

## Severity Assessment

| Category | Severity | Notes |
|----------|----------|-------|
| Dead code | N/A | No code exists |
| Disconnected modules | **Critical** | Entire project is scaffolding only |
| Stubs/unfinished | **Critical** | 100% of promised features are unimplemented |
| Config not enforced | Low | Config is valid but vacuous |
| Tech debt | Medium | README is actively misleading |

**Overall**: This isn't a house with unused rooms — it's a foundation with no house. The Winchester Mystery House analogy almost doesn't apply because there's nothing mysterious: it's plainly empty. The only "mystery" is why the scaffolding exists without any implementation.
