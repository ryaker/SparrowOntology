# Pagination Support for SparrowOntology MCP Tools

## Overview

The SparrowOntology MCP tool now provides comprehensive pagination support for list-returning endpoints. This document describes the pagination API for the `find_entities` and `get_ontology` tools.

## Pagination Metadata

All paginated responses include metadata to help clients navigate through results:

```json
{
  "total_count": 100,      // Total number of items across all pages
  "offset": 0,             // Current offset in the result set
  "limit": 20,             // Items returned per page
  "has_more": true,        // Whether more results exist
  "next_cursor": "..."     // Opaque cursor for next page (if has_more=true)
}
```

## Find Entities Tool

The `find_entities` tool supports both **cursor-based** and **offset-based** pagination.

### Parameters

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `class_name` | string | required | Class name to find entities of |
| `include_subclasses` | boolean | false | Include entities from subclasses |
| `limit` | integer | 20 | Number of results per page |
| `offset` | integer | 0 | Skip N results (offset-based pagination) |
| `cursor` | string | - | Opaque cursor from previous response (cursor-based) |
| `filters` / `where` | object | - | Property filters for results |

### Response Format

```json
{
  "entities": [
    {
      "node_id": "123",
      "label": "Person",
      "properties": { "name": "Alice", ... }
    },
    ...
  ],
  "pagination": {
    "total_count": 50,
    "offset": 0,
    "limit": 20,
    "has_more": true,
    "next_cursor": "b2Zmc2V0OjIw"  // Opaque base64-encoded cursor
  }
}
```

### Usage Examples

#### Offset-Based Pagination (Simple)

```json
{
  "class_name": "Person",
  "limit": 10,
  "offset": 0
}
```

Response includes `pagination.next_cursor` if more results exist:

```json
{
  "limit": 10,
  "offset": 0,
  "total_count": 25,
  "has_more": true,
  "next_cursor": "b2Zmc2V0OjEw"
}
```

#### Cursor-Based Pagination (Recommended)

Cursor-based pagination is **stable** across concurrent updates:

**First request:**
```json
{
  "class_name": "Person",
  "limit": 10
}
```

**Next request (use cursor from first response):**
```json
{
  "class_name": "Person",
  "limit": 10,
  "cursor": "b2Zmc2V0OjEw"
}
```

Cursors are **opaque** — treat them as black boxes. The format may change in future versions.

#### With Filters

Pagination works with property filters:

```json
{
  "class_name": "Person",
  "limit": 10,
  "filters": {
    "age": 30,
    "city": "San Francisco"
  }
}
```

Filters are applied **before** pagination, so `total_count` reflects filtered results only.

### Pagination Strategy

1. **Use cursor-based pagination** by default — it's stable across updates
2. **Use offset-based** for simple sequential access (first page, then second page, etc.)
3. **Don't mix** cursor and offset in the same sequence
4. **Check `has_more`** before requesting next page
5. **Default limit is 20** — tune for your use case (5–100 typical range)

## Get Ontology Tool

The `get_ontology` tool returns the complete ontology schema with **separate pagination for each section** (classes, relations, properties, aliases).

### Parameters

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `class_limit` | integer | 50 | Classes per page |
| `class_offset` | integer | 0 | Classes offset |
| `relation_limit` | integer | 50 | Relations per page |
| `relation_offset` | integer | 0 | Relations offset |
| `property_limit` | integer | 50 | Properties per page |
| `property_offset` | integer | 0 | Properties offset |
| `alias_limit` | integer | 50 | Aliases per page |
| `alias_offset` | integer | 0 | Aliases offset |

### Response Format

Each section is now wrapped in a `data` and `pagination` structure:

```json
{
  "classes": {
    "data": [
      {
        "name": "Person",
        "description": "A human",
        "properties": [...],
        ...
      },
      ...
    ],
    "pagination": {
      "total_count": 125,
      "offset": 0,
      "limit": 50,
      "has_more": true
    }
  },
  "relations": {
    "data": [...],
    "pagination": {...}
  },
  "properties": {
    "data": [...],
    "pagination": {...}
  },
  "aliases": {
    "data": [...],
    "pagination": {...}
  }
}
```

### Usage Examples

#### Fetch First 20 Classes

```json
{
  "class_limit": 20,
  "class_offset": 0
}
```

#### Fetch Large Ontology in Chunks

```json
{
  "class_limit": 50,
  "relation_limit": 50,
  "property_limit": 100,
  "alias_limit": 100
}
```

Response tells you if more data exists per section:

```json
{
  "classes": {
    "pagination": {
      "total_count": 250,
      "has_more": true
    }
  },
  ...
}
```

#### Load Everything (Default)

```json
{}
```

Uses 50-item defaults for all sections. Small ontologies fit in one request.

## Best Practices

### Performance

- **Use reasonable limits**: 20–100 items is typical. Larger limits = fewer requests but bigger responses.
- **Prefer cursor pagination**: It's immune to concurrent inserts/deletes affecting offset calculations.
- **For large ontologies**: Fetch `get_ontology` in sections using pagination instead of fetching the whole thing at once.

### Client Patterns

```python
# Cursor-based pagination (recommended)
cursor = None
all_entities = []

while True:
    params = {"class_name": "Person", "limit": 50}
    if cursor:
        params["cursor"] = cursor

    response = call("find_entities", params)
    all_entities.extend(response["entities"])

    if not response["pagination"]["has_more"]:
        break

    cursor = response["pagination"]["next_cursor"]
```

```python
# Offset-based pagination (simple)
offset = 0
all_entities = []

while True:
    response = call("find_entities", {
        "class_name": "Person",
        "limit": 50,
        "offset": offset
    })

    all_entities.extend(response["entities"])

    if not response["pagination"]["has_more"]:
        break

    offset += 50
```

## Implementation Notes

### Cursor Encoding

Cursors are hex-encoded strings in the format `offset:<N>`. They're stable and can be stored/transmitted safely as strings.

### Backward Compatibility

- Existing `find_entities` calls without pagination parameters work as before
- Default `limit=20` and `offset=0` ensure familiar behavior
- `get_ontology` responses now include pagination metadata—clients ignoring it continue to work

### Database Queries

- Filters in `find_entities` are applied in **Rust after query results**, not in Cypher (SparrowDB limitation)
- Pagination happens **after filtering**, so `total_count` reflects filtered results
- `get_ontology` counts are computed from in-memory results, not database queries

## Troubleshooting

### "Cursor is invalid"

Cursors are opaque and version-specific. Don't store them long-term or between server versions.

### "Total count doesn't match expected"

For `find_entities`, `total_count` is the count **after applying filters**. If you applied filters, the count reflects the filtered set only.

### Large Ontologies Timing Out

Use pagination to fetch `get_ontology` in smaller chunks:

```json
{
  "class_limit": 50,
  "relation_limit": 50,
  "property_limit": 50,
  "alias_limit": 50
}
```

Then fetch subsequent pages as needed.

## Migration Guide

If you're currently using unpaginated calls:

**Before:**
```json
{
  "class_name": "Person"
}
```

**Response had:**
```json
{
  "entities": [...]  // Could be hundreds of items
}
```

**After:**
```json
{
  "class_name": "Person"
}
```

**Response now has:**
```json
{
  "entities": [...],  // Default 20 items
  "pagination": {
    "total_count": 500,
    "offset": 0,
    "limit": 20,
    "has_more": true,
    "next_cursor": "..."
  }
}
```

To get all results, loop using `next_cursor` or increment `offset` until `has_more=false`.
