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
