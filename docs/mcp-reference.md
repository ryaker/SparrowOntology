# MCP Tool Reference

All 17 tools exposed by `sparrow-ontology-mcp`.

---

## Operational

### `health`

Operational ping. Call before any write session.

**Parameters:** none

**Response:**
```json
{ "status": "ok", "db_path": "/Users/ryaker/sparrow-ontology.db" }
```

If `status` is not `"ok"`, stop. Do not attempt writes. Restart the service and retry.

Also available via HTTP: `GET /health`

---

### `stats`

Schema analytics and entity counts.

**Parameters:** none

**Response:**
```json
{
  "classes": 5,
  "relations": 8,
  "properties": 12,
  "entities": {
    "Person": 14,
    "Project": 6,
    "Task": 47,
    "total": 67
  }
}
```

Also available via HTTP: `GET /ontology/stats`

---

## Schema — Read

### `start_here`

Schema orientation. Always call first in a session.

**Parameters:**
- `template` (optional): `"WorldModel"` | `"PersonalKnowledge"` | `"ProfessionalNetwork"` | `"ResearchNotes"`

**Response:**
```json
{
  "initialized": true,
  "class_count": 10,
  "relation_count": 19,
  "property_count": 22,
  "unseeded_classes": ["Event", "Location", "Project"],
  "template": "WorldModel"
}
```

`unseeded_classes` are classes with 0 declared properties. You can create bare entities for them, but any property write will fail until `add_property` is called.

---

### `get_ontology`

Full schema dump.

**Parameters:** none

**Response:** all classes, relations, properties, aliases in full detail.

---

### `explain_symbol`

Full detail on a single class or relation.

**Parameters:**
- `name` (required): class or relation name
- `kind` (optional): `"class"` | `"relation"` — helps disambiguate if names collide

**Response:**
```json
{
  "name": "Person",
  "kind": "class",
  "properties": [
    { "name": "name", "datatype": "string", "required": true },
    { "name": "email", "datatype": "string", "required": false }
  ],
  "aliases": ["person", "human"],
  "subclasses": ["Employee"],
  "superclasses": []
}
```

Use before writing subclass entities — shows the full inherited property chain.

---

### `resolve_name`

Resolve an alias to its canonical symbol.

**Parameters:**
- `name` (required): alias or canonical name

**Response:**
```json
{ "canonical": "Organization", "was_alias": true }
```

---

## Schema — Write

### `define_class`

Add a new entity type.

**Parameters:**
- `name` (required): class name. Cannot start with `__SO_`.
- `description` (optional): human-readable description

**Response:**
```json
{ "created": true, "name": "Employee" }
```

---

### `define_relation`

Add a typed relation with domain/range constraints.

**Parameters:**
- `name` (required): relation name, conventionally `UPPER_SNAKE_CASE`
- `domain` (required): source class name
- `range` (required): target class name

**Response:**
```json
{ "created": true, "name": "REPORTS_TO", "domain": "Person", "range": "Person" }
```

---

### `define_subclass`

Create a subclass relationship. Cycle detection built in.

**Parameters:**
- `child` (required): child class name
- `parent` (required): parent class name

**Response:**
```json
{ "created": true, "child": "Employee", "parent": "Person" }
```

The child class now inherits all `required: true` properties from the parent (and all ancestors).

---

### `define_subproperty`

Create a subproperty relationship.

**Parameters:**
- `child` (required): child property name
- `parent` (required): parent property name

---

### `add_property`

Declare a typed property on a class.

**Parameters:**
- `owner` (required): class name — **NOT** `class_name`
- `name` (required): property name — **NOT** `property_name`
- `datatype` (optional): `"string"` | `"integer"` | `"float"` | `"boolean"` (default: `"string"`)
- `required` (optional): boolean (default: `false`)

**Response:**
```json
{ "created": true, "owner": "Person", "name": "email", "datatype": "string", "required": false }
```

**Common mistake:** using `class_name` or `property_name` instead of `owner` and `name`. The tool will return a missing param error.

---

### `add_alias`

Register a spelling alias for a class or relation.

**Parameters:**
- `alias_name` (required): the alias (e.g. `"org"`)
- `target` (required): canonical name (e.g. `"Organization"`)
- `kind` (required): `"class"` | `"relation"`

**Response:**
```json
{ "created": true, "alias": "org", "target": "Organization" }
```

Aliases are persistent. Once registered, they apply to all future sessions.

---

## Data

### `create_entity`

Write a validated entity. Schema checked before storage.

**Parameters:**
- `class_name` (required): class name (or alias — will be resolved)
- `properties` (optional): object of property name → value pairs

**Response:**
```json
{ "node_id": "4294967296", "canonical_label": "Person", "created": true }
```

`node_id` is a numeric string. Save it for `create_relationship` calls.

---

### `update_entity`

Update properties on an existing entity.

**Parameters:**
- `node_id` (required): numeric string from `create_entity`
- `properties` (required): object of property name → new value

**Response:**
```json
{ "node_id": "4294967296", "updated": true }
```

---

### `create_relationship`

Write a domain/range-validated edge between two entities.

**Parameters:**
- `from_id` (required): source node_id (numeric string)
- `to_id` (required): target node_id (numeric string)
- `relation_name` (required): relation name (or alias)
- `properties` (optional): edge properties

**Response:**
```json
{ "created": true, "relation": "WORKS_FOR" }
```

---

### `find_entities`

Query entities by class and optional property filters.

**Parameters:**
- `class_name` (required): class to query
- `filters` (optional): object of property name → value for equality filters
- `limit` (optional): max results (default: no limit)

**Response:**
```json
[
  { "node_id": "4294967296", "label": "Person", "properties": { "name": "Alice", "email": "alice@example.com" } },
  ...
]
```

---

### `validate`

Dry-run schema validation without writing.

**Parameters:**
- `class_name` (required): class to validate against
- `properties` (required): object of property name → value

**Response on success:**
```json
{ "valid": true }
```

**Response on failure:**
```json
{
  "valid": false,
  "error": "Unknown property 'role'. Valid: [\"name\", \"email\"]. Call add_property(owner='Person', name='role') to declare it."
}
```

Use `validate` in agent planning phases before committing to a write sequence.

---

## Data — RDF (instance data)

These export and import the *entities and relationships*, as opposed to
`export_json_ld` / `import_turtle` which handle the *schema*. Full detail,
including the datatype mapping table and IRI minting rules, is in
[RDF Data Export](rdf-data-export.md).

### `export_data_turtle`

Export every entity and relationship as Turtle.

**Parameters:**
- `base_iri` (required): namespace entity IRIs are minted under, e.g. `https://example.org/kb`

**Response:** two content blocks — the Turtle document, then a JSON report:

```json
{
  "entities_exported": 2,
  "relationships_exported": 1,
  "triples_emitted": 6,
  "orphan_properties": 0,
  "orphan_labels": [],
  "orphan_relation_types": [],
  "dangling_edges": 0,
  "unrepresentable_iris": 0,
  "issues": []
}
```

`unrepresentable_iris` counts classes or entities whose name could not form a
valid IRI (e.g. a class name containing a space) — skipped and named in
`issues` rather than aborting the export.

Entities that carry a stored IRI from a previous import keep it; the rest are
minted as `{base_iri}/{ClassName}/{node_id}`. Export is read-only.

### `export_data_json_ld`

The same graph in JSON-LD 1.1. The `@context` is derived from the ontology:
relations become `"@type": "@id"` terms, and typed properties carry XSD datatype
coercions.

**Parameters:**
- `base_iri` (required)

### `import_data_turtle`

Import instance data through the validated write path. The subject IRI is
persisted on each entity, so a later export reproduces it rather than minting a
new one.

**Parameters:**
- `turtle` (required): the Turtle instance-data text
- `strategy` (optional, default `strict`): `strict` rejects unknown classes,
  relations, and properties; `auto_declare` creates them, deriving property
  types from XSD datatypes and relation domain/range from the observed classes

**Response:**
```json
{
  "entities_imported": 2,
  "relationships_imported": 1,
  "entities_skipped": 1,
  "relationships_skipped": 0,
  "blank_nodes_skipped": 0,
  "classes_declared": 0,
  "relations_declared": 0,
  "properties_declared": 0,
  "skips": [
    {
      "subject": "https://example.org/kb/Person/2",
      "reason": "Unknown class <https://example.org/kb/schema/Wizard>. Valid: [\"Concept\", \"Organization\", \"Person\"]. Declare it with define_class, or re-run with ImportStrategy::AutoDeclare."
    }
  ],
  "warnings": []
}
```

Nothing is dropped silently — every skipped subject appears in `skips` with its
IRI and an actionable reason. Blank nodes are counted and skipped (blank-node
preservation is out of scope).

**Partial failure:** the tool call's top-level result carries `"isError":
true` whenever anything was skipped (`entities_skipped + relationships_skipped
+ blank_nodes_skipped > 0`), matching the CLI's `cmd_import_data`, which
treats any skip as a pipeline-gate failure. A client that checks `isError`
before parsing `content` — the documented MCP pattern — sees an import that
skipped everything as a failure, not a clean-looking success.
