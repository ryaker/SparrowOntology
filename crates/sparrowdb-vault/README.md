# sparrowdb-vault

The Vault-LD engine for Sparrow Ontology (integration spec **WS2**).

[Vault-LD](https://vault-ld.org/) (spec v0.5.0, Apache-2.0) defines how a folder of
Markdown notes reads as an RDF graph: YAML frontmatter is YAML-LD resolved through a
composed `context.jsonld`, `[[Wiki Links]]` are edges, bodies stay prose. It ships a
format and two stateless batch scripts (`vault_to_rdf.py`, `rdf_to_vault.py`). It has no
database, write-time validation, incremental sync, query interface, or drift detection.
This crate adds those on top of SparrowOntology + SparrowDB.

> **Status: scaffold.** Module layout, public types, and CLI surface are in place.
> Every operation that reads a vault or touches a DB returns
> `VaultError::NotImplemented { operation, slice }`. The only real code is the
> dependency-free parsing primitives (frontmatter split, wiki-link grammar,
> layer classification, scoped-base lookup), which have unit tests.
> `publish = false` until the first functional slice lands.

## Mapping Vault-LD concepts to modules

| Vault-LD concept | Spec | Module | Sparrow target |
|---|---|---|---|
| Vault tree; schema layer (`Ontologies/`, `Vocabularies/`) vs instance layer | §3, §5.1, §5.4.5 | `layout` | — |
| Composed `context.jsonld`, keyword aliases, scoped `@base`, shadow warnings | §4.2, §4.3 | `context` | — |
| Frontmatter block; `[[path/name#frag\|alias]]` grammar | §4.1, §4.4.1 | `parse` | — |
| Identity: `@base` + percent-encoded file name, or absolute `id` | §4.5 | `identity` | entity `__so_iri` (WS1 already prefers it on export) |
| `owl:Class` note | §5.1 | `sync` | `define_class` (+ `define_subclass` from `subClassOf`) |
| `owl:ObjectProperty` note | §5.1 | `sync` | `define_relation` (domain/range) |
| `owl:DatatypeProperty` note | §5.1 | `sync` | `add_property` (source IRI, #40) |
| Instance note `type: "[[Recipe]]"` | §4.6 | `sync` | validated entity; body → `_body` text property |
| Wiki-link value on an instance | §4.4 | `sync` | validated relationship |
| DB → notes, in place, bodies/tags/folders preserved | §5.5, §6 | `export` | reads WS1 export model |
| Conformance lints (dangling link, relative `id`, inline `@context`, prefixed keys) | §4.2–§4.5, §6 | `check` | plus ontology validation errors |
| Vault vs DB divergence | — (Sparrow addition) | `drift` | sync manifest |
| Incremental re-sync | — (Sparrow addition) | `watch` | `notify`, feature-gated |
| `owl:Ontology`, `skos:ConceptScheme`, `skos:Concept` | §5.1 | `layout::NoteKind` | **no counterpart yet** (open question) |
| `vld:path` placement triple | §5.4.7 | `export` | **no counterpart yet** (open question) |

## CLI: `sparrow-ontology vault …` (not a separate `sparrow-vault` binary)

```
sparrow-ontology vault sync   <vault> --db <path> [--vault-authoritative] [--hard-delete]
sparrow-ontology vault watch  <vault> --db <path> [--hard-delete]
sparrow-ontology vault export <vault> --db <path> [--dry-run]
sparrow-ontology vault check  <vault> --db <path> [--json]
sparrow-ontology vault drift  <vault> --db <path> [--json]
```

Why subcommands on the existing CLI:

1. **One install, one mental model.** The vault is another projection of the same
   ontology-governed DB that `export-data`, `import-turtle`, and `export-json-ld` already
   project. A second binary would duplicate `--db` handling, error rendering, and release
   plumbing for no user-visible gain. Spec §2.8: boring to operate.
2. **The library stays CLI-free.** All logic is in this crate; the CLI side is
   `crates/sparrowdb-ontology-cli/src/vault.rs`, a thin clap layer. A `sparrow-vault`
   binary can still be added later as a ~20-line wrapper if packaging needs it.
3. **Heavy deps stay optional.** `watch` needs `notify`; it goes behind a cargo feature,
   so the CLI doesn't pull a file watcher unless built with it.

Tradeoff: `sparrowdb-ontology-cli` now depends on a `publish = false` crate. That doesn't
change anything today, since the workspace already can't publish while
`[patch.crates-io]` pins SparrowDB to a git rev. It has to be resolved before #33.

## WS1 dependency status (as of `origin/main` f2142d1, 2026-09-14)

WS1 is **merged** (#43 instance-data RDF export/import, #40 datatype-property metadata):
`export_data_turtle`, `export_data_json_ld`, `export_data_with_report`,
`import_data_turtle`, plus CLI `export-data` / `import-data`. Workspace build and tests
pass. The scaffold doesn't need anything else. The WS2 conformance harness does:

| Gap | Blocks | Notes |
|---|---|---|
| No **Turtle schema export**. Schema only leaves as JSON-LD (`export_json_ld`) | isomorphism test | rdflib can parse the JSON-LD. Nobody has checked that its triples match `vault_to_rdf.py`'s `schema.ttl` (labels, comments, `owl:DatatypeProperty`, domain/range). |
| **Scoped bases**: `export_data_turtle(base_iri)` takes one base | instance IRIs | Fine if sync writes `__so_iri` on every entity. Class IRIs come from `define_class --iri`. Class `source_iri` is still open (#42). |
| **Local-name collisions** across ontologies | multi-ontology vaults | PR #46 open |
| Export not snapshot-consistent | `drift` correctness | #44 open |
| rdflib in CI | isomorphism test infra | PR #45 open |
| SparrowDB pinned by **git rev** in `[patch.crates-io]` | publishing only | Not a build blocker |
| No model for **SKOS concepts / `owl:Ontology` / `vld:path` / external CURIE objects** (`subClassOf: [sdo:Recipe]`) | full-fidelity sync | Needs a design decision, see below |

## Open questions

Spec §7 defaults, adopted unless someone overrides them:

- **Schema merge on sync:** default `SchemaMergePolicy::MergeWithWarning`. Vault schema
  notes merge into the existing ontology, and every conflict is reported, never silently
  overwritten. `--vault-authoritative` opts into vault-wins.
- **Binary/asset files** (images, PDFs): ignored, logged at debug level, not indexed.

Found while scaffolding, still undecided:

- **SKOS vocabularies.** Model `skos:Concept` as entities of a reserved class, or flag the
  whole layer as out of scope for v1 (§5.6 allows flag-don't-drop)?
- **`vld:path` storage.** Use a reserved entity property (`__vld_path`), or derive the
  path from the sync manifest?
- **External IRI objects** (`sdo:Recipe` as a superclass). There is no ontology node to
  point at. Options: auto-declare a stub class carrying the IRI, or keep them as
  flagged, unmapped triples.
- **Drift manifest location.** A sidecar file in the vault (diff-friendly, but it pollutes
  the vault) or reserved nodes in the DB (invisible to git)?
- **YAML dialect.** The reference exporter uses PyYAML 6 (YAML **1.1**) with a custom
  loader. YAML 1.1 and 1.2 disagree on `yes`/`no`/`on`/`off` and on implicit timestamps,
  so byte-level isomorphism depends on matching 1.1 scalar resolution. `serde_yaml` is
  archived. Candidates are `saphyr` (1.2, has source markers for line-numbered
  diagnostics) and `serde_norway`. Decide in the parse slice.
- **Frontmatter delimiting.** `vault_to_rdf.py` splits on the first two raw `---`
  substrings. This crate matches whole `---` lines, which is correct YAML. The two
  diverge on a value that contains `---`. Record it as a known deviation, or mirror the
  reference?
- **Frontmatter size cap.** The reference skips oversized frontmatter
  (`MAX_FRONTMATTER_BYTES`). Mirror that as a DoS guard (cf. #19).

## Conformance

`tests/conformance/` holds a placeholder harness pinned to upstream vault-ld commit
`025e71be8d810e387dc451c920e05472d1c47170`. Acceptance tests are already named there and
`#[ignore]`d. Upstream fixtures are fetched, not vendored (MIT/Apache hygiene, spec §2.7).
The goal is **eventual graph isomorphism** with `vault_to_rdf.py`, plus byte-stable
`sync → export` idempotence.

## Out of scope for this crate (spec non-goals)

Editing bodies from the graph side, an Obsidian plugin, multi-vault federation, git
integration, inline (non-frontmatter) annotations, a SPARQL engine, and MCP tools
(a later phase).
