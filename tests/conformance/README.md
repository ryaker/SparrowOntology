# Vault-LD conformance harness (placeholder)

Target: **eventual graph isomorphism** between `sparrowdb-vault` and the upstream
reference converters of [Vault-LD](https://github.com/The-Knowledge-Graph-Guys/vault-ld)
spec v0.5.0, pinned at upstream commit `025e71be8d810e387dc451c920e05472d1c47170`.

Tests live in `test_vault_conformance.rs`. Parse-slice `check` tests against
seeded vaults under `vaults/` are live; sync/export/isomorphism tests stay
`#[ignore]`d until their slice lands.

## Fixtures — fetched, not vendored

The upstream `Vault-LD Example/` vault and `scripts/vault_to_rdf.py` are Apache-2.0.
Sparrow stays MIT and does not vendor their code (spec §2.7). Plan:

- A CI step (and a `tests/tools/fetch_vault_ld.sh` helper, not yet written) does a
  shallow fetch of the pinned rev into `target/vault-ld/`, which is git-ignored.
- Tests locate fixtures via `VAULT_LD_FIXTURE_DIR`; when unset they skip with a
  clear message, so `cargo test` stays offline-clean.
- Attribution for the example content goes in this README when the fetch lands.

Seeded-error vaults for `check` (unknown property, dangling link,
malformed YAML) are our own content and *can* be committed here under
`tests/conformance/vaults/`.

## Isomorphism check

WS1 already runs `rdflib` in CI via `tests/tools/verify_rdf.py`. The comparison is:

```
vault_to_rdf.py "Vault-LD Example" --out-dir ref/        # schema.ttl + data.ttl
sparrow-ontology vault sync "Vault-LD Example" --db tmp.db
sparrow-ontology export-data --db tmp.db --format ttl ...  # data layer
sparrow-ontology export-json-ld --db tmp.db               # schema layer (JSON-LD; no Turtle schema export yet — see crate README "WS1 dependency status")
rdflib.compare.isomorphic(ref_union, ours_union)
```

## TODO

- [ ] `fetch_vault_ld.sh` + CI step pinned to the rev above
- [ ] decide how `vld:path` triples are compared (roundtrip-face export only)
- [ ] un-ignore `example_vault_isomorphic_to_reference_exporter`
- [ ] un-ignore `sync_export_roundtrip_is_idempotent`
- [ ] un-ignore `check_reports_three_seeded_error_kinds` (needs UnknownProperty / RelationRangeViolation)
- [x] seeded-error vaults (`clean`, `dangling-link`, `relative-id`, `malformed-yaml`, `unknown-property`)
- [x] un-ignore `malformed_frontmatter_is_skipped_not_fatal` (via `check`; sync reuse later)
