# Sparrow Ontology

An ontology semantic layer on top of SparrowDB. Provides semantic alias normalization, write-time validation, hierarchy expansion, and a guided world-model bootstrap.

## Building

```bash
cargo build --workspace
```

## Testing

```bash
cargo test --workspace
```

## Phase 1 Status

Phase 1 (`sparrowdb-ontology-core`) implements the ontology metadata model, alias resolution, validation engine, hierarchy expansion, and world model bootstrap.

All 24 acceptance criteria must pass before Phase 2 (MCP server) begins.
