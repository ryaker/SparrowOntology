# Instance-data RDF export and import

`export_json_ld` projects the ontology **schema** (`owl:Class`,
`owl:ObjectProperty`, …). This document covers the complementary half: exporting
and importing the **instance data** — the actual entities and relationships.

Together they match the two-file convention Vault-LD uses: `schema.ttl` from the
schema exporter, `data.ttl` from this one.

| Surface | Export Turtle | Export JSON-LD | Import Turtle |
|---|---|---|---|
| Rust | `export_data_turtle(&db, base)` | `export_data_json_ld(&db, base)` | `import_data_turtle(&db, ttl, strategy)` |
| MCP | `export_data_turtle` | `export_data_json_ld` | `import_data_turtle` |
| CLI | `export-data --format ttl` | `export-data --format jsonld` | `import-data <file.ttl>` |

`export_data_with_report(&db, base)` returns the Turtle *and* an `ExportReport`
describing everything that could not be represented. The MCP tool and the CLI
both surface it.

## Quickstart

```console
$ sparrow-ontology init --db ./kb
Initialized: 10 classes, 19 relations, 22 properties

$ sparrow-ontology create-entity Person --db ./kb \
    --props '{"name":"Ada Lovelace","email":"ada@acme.example"}'
Created entity: Person  node_id=12884901888

$ sparrow-ontology create-entity Organization --db ./kb --props '{"name":"Acme Corp"}'
Created entity: Organization  node_id=17179869184

$ sparrow-ontology create-relationship --db ./kb \
    --from 12884901888 --rel-type WORKS_FOR --to 17179869184
Created relationship: (12884901888)-[:WORKS_FOR]->(17179869184)

$ sparrow-ontology export-data --db ./kb --format ttl --base https://example.org/kb
@prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .
@prefix xsd: <http://www.w3.org/2001/XMLSchema#> .
<https://example.org/kb/Organization/17179869184> a <https://example.org/kb/schema/Organization> ;
	<https://example.org/kb/schema/name> "Acme Corp" .
<https://example.org/kb/Person/12884901888> a <https://example.org/kb/schema/Person> ;
	<https://example.org/kb/schema/WORKS_FOR> <https://example.org/kb/Organization/17179869184> ;
	<https://example.org/kb/schema/email> "ada@acme.example" ;
	<https://example.org/kb/schema/name> "Ada Lovelace" .

Export complete:
  Entities:      2
  Relationships: 1
  Triples:       6
```

The graph goes to stdout (or `--out`); the report goes to stderr, so `--out`
produces a clean RDF file and a stdout pipe stays parseable.

## Datatype mapping

Ontology `PropertyType` → XSD datatype emitted on export:

| `PropertyType` | Emitted datatype | Also accepted on import |
|---|---|---|
| `String` | `xsd:string` (plain literal) | anything unrecognised |
| `Int64` | `xsd:integer` | `int`, `long`, `short`, `byte`, `nonNegativeInteger`, `positiveInteger`, `negativeInteger`, `nonPositiveInteger`, `unsignedLong`, `unsignedInt`, `unsignedShort`, `unsignedByte` |
| `Float64` | `xsd:double` | `decimal`, `float` |
| `Bool` | `xsd:boolean` | — |
| `Date` | `xsd:date` **or** `xsd:dateTime` | both |
| `Variant` | `xsd:string` | — |

This table is the exact inverse of `xsd_to_type_str` in `turtle_import.rs`. The
two live in different modules, so they are pinned together by the
`datatype_mapping_is_a_fixed_point` test, which round-trips every
`PropertyType` through export → `xsd_to_type_str` → `property_type_from_str` and
asserts the result is unchanged. Editing one side without the other fails that
test.

### `xsd:date` vs `xsd:dateTime`

`PropertyType::Date` covers both. Dates are stored as plain strings
(`PropertyValue` has no `Date` variant — "dates stored as strings in v1"), and
`xsd_to_type_str` maps both `xsd:date` and `xsd:dateTime` onto `date`.

Emitting a stored `2026-03-01T09:30:00Z` as `^^xsd:date` would be an **ill-typed
literal** — a malformed lexical form for that datatype. So the exporter picks the
datatype from the value: a lexical form containing `T` is emitted as
`xsd:dateTime`, otherwise `xsd:date`. Date round-trips are therefore exact, and
every emitted literal is well-formed.

### Known lossy edge

`Variant` has no XSD counterpart and is exported as `xsd:string`. Re-importing
yields `PropertyType::String`, not `Variant`. This is the only type that does not
survive a round-trip.

## IRI minting and round-trip stability

An entity's subject IRI is, in priority order:

1. the IRI stored on the entity in the reserved `__so_iri` property;
2. otherwise minted as `{base_iri}/{ClassName}/{node_id}`.

`import_data_turtle` **persists** the subject IRI it read onto each entity it
creates. That is what makes `export → wipe → import → re-export` reproduce the
same subject IRIs: minting only ever happens on the first export of a
never-imported entity.

Without this the round-trip could not be isomorphic — re-import assigns fresh
node ids, so minted IRIs would change and the second export would describe a
different graph. This mirrors how `OntologyClass` and `OntologyRelation` already
carry optional explicit `iri` fields.

**Export is strictly read-only.** It never writes minted IRIs back.

`node_id` is the full label-scoped `NodeId` (`(label_id << 32) | slot`), never
the bare slot: Organization slot 0 and Person slot 0 are different nodes with ids
`0` and `4294967296`.

### Class, relation, and property IRIs

| Term | IRI |
|---|---|
| Class | its declared `iri`, else `{base}/schema/{ClassName}` |
| Relation | its declared `iri`, else `{base}/schema/{RELATION_NAME}` |
| Property | its `source_iri`, else `{base}/schema/{propertyName}` |

The fallback is derived from the **name**, not the `symbol_id`. The schema
exporter falls back to `so:{symbol_id}`, but `symbol_id` is a fresh UUID on every
`init`, so a `symbol_id`-derived IRI is not stable across a schema replay and
would break round-trip isomorphism.

The practical consequence: declare explicit IRIs on your classes and relations
(`define-class --iri …`, or import a vocabulary with `import_turtle`) and
`data.ttl` and `schema.ttl` will agree exactly. Without them the two files use
different IRIs for the same class.

Property predicates are minted from the property name alone, not scoped by
owning class. This mirrors how RDF vocabularies work — `foaf:name` is one
predicate regardless of subject class — and it lets import resolve a predicate
without knowing the subject's class first.

## Only ontology-declared properties are exportable

SparrowDB stores property names as `col_<fnv1a32(name)>` column keys, and **that
hash is one-way**. To read a property back by name, the exporter builds a reverse
table: it takes every property the ontology declares, hashes each name, and
matches the resulting `col_` key.

A property written directly through the low-level `WriteTx` API without a
matching ontology declaration therefore cannot be recovered by name, and cannot
be exported. Such columns are **counted and reported** as
`ExportReport::orphan_properties`, with an issue naming the subject and the fix:

```text
Stored column 'col_1234567890' has no ontology-declared property on class
'Person'. Property names hash one-way to col_<fnv1a32>, so this value cannot be
named in RDF. Declare it with add_property(owner='Person', …) and re-export.
```

## Import strategies

- **`strict`** (default) — reject unknown classes, relations, and properties.
  The offending subject is skipped and reported; the ontology is not modified.
- **`auto-declare`** — declare unknown classes, relations, and properties on the
  fly. Property types are derived from the literal's XSD datatype; relation
  domain and range from the observed subject and object classes.

Nothing is ever dropped silently. Every skip is recorded with the offending
subject IRI and an actionable reason, in the same style as `create_entity`
errors:

```text
Unknown property 'favouriteColour' (from <…/schema/favouriteColour>) on class
'Person'. Valid: ["email", "name"]. Declare it with
add_property(owner='Person', name='favouriteColour'), or re-run with
ImportStrategy::AutoDeclare.
```

The CLI exits nonzero if anything was skipped, so `import-data` works as a
pipeline gate.

## Non-goals

Out of scope for this layer, by design:

- **SPARQL query engine** — use Cypher, or export and query elsewhere.
- **Named graphs** — the export is a single default graph.
- **Blank-node preservation on import** — blank nodes are counted in
  `ImportReport::blank_nodes_skipped` and skipped, never silently dropped
  (Vault-LD §5.6 spirit). The exporter never emits blank nodes, so exported
  graphs are ground.
- **Reasoning / inference** — no subclass materialisation, no owl:sameAs
  resolution. What you wrote is what you get.

Language tags on imported literals are dropped (v1 stores plain strings) and each
occurrence is recorded in `ImportReport::warnings`.

## Verifying an export

Exported graphs are checked by an independent RDF implementation, not only by the
oxrdf library that produced them. `tests/tools/verify_rdf.py` is a **test-only**
script (no runtime dependency) that parses both Turtle exports and the JSON-LD
through rdflib and asserts pairwise isomorphism:

```console
$ python3 tests/tools/verify_rdf.py export1.ttl export2.ttl export1.jsonld
parsed: export1.ttl=42 triples, export2.ttl=42 triples, export1.jsonld=42 triples
OK: exports are graph-isomorphic (Turtle round-trip and JSON-LD)
```

The integration test `roundtrip_is_graph_isomorphic` runs it automatically when
rdflib is importable. Point `SPARROW_RDFLIB_PYTHON` at an interpreter that has
rdflib to enable it in CI; without it the script exits 77 and the test reports
the cross-check as skipped rather than passing silently.

## Engine caveats

As of SparrowDB 0.1.27 (the version this crate pins), the engine refuses to
create dangling edges rather than leaving them behind:

- `MATCH (n:Label) DELETE n` on a node with edges is refused outright
  (`NodeHasEdges`) instead of silently removing the node and leaving its edges
  behind.
- `DETACH DELETE` removes the node and its edges atomically.

Both were bugs on SparrowDB 0.1.22 and earlier — `DELETE` left dangling edges,
and `DETACH DELETE` failed outright with `relationship type
'__SO_HAS_PROPERTY' not found in catalog`. Neither is reachable through this
crate's supported engine version, but the exporter still **skips** edges
whose endpoints no longer exist and counts them in
`ExportReport::dangling_edges`, as defense-in-depth for a database written by
an older engine.

To wipe instance data reliably today, create a fresh database and replay the
schema rather than deleting in place.
