//! Vault-LD conformance harness — PLACEHOLDER (WS2 deliverable 5).
//!
//! Every test here is `#[ignore]` until the slice it depends on lands. They
//! exist so the acceptance criteria have named homes from day one. See
//! `tests/conformance/README.md` for fixture sourcing and the plan.

/// Upstream commit the conformance fixtures and reference exporter are pinned to.
const VAULT_LD_UPSTREAM_REV: &str = "025e71be8d810e387dc451c920e05472d1c47170";

#[test]
fn pinned_upstream_rev_is_a_full_sha() {
    assert_eq!(VAULT_LD_UPSTREAM_REV.len(), 40);
    assert_eq!(
        sparrowdb_vault::VAULT_LD_SPEC_VERSION,
        "0.5.0",
        "bump the pinned upstream rev together with the spec version"
    );
}

/// TODO(ws2/conformance): `sync` the upstream `Vault-LD Example/` vault, then
/// WS1 `export_data_turtle` + a schema Turtle export, and assert the union is
/// graph-isomorphic to `vault_to_rdf.py` output on the same vault.
#[test]
#[ignore = "needs sync + schema-layer RDF export parity; see tests/conformance/README.md"]
fn example_vault_isomorphic_to_reference_exporter() {
    unimplemented!()
}

/// TODO(ws2/conformance): `sync → export` twice on the example vault; the second
/// run must be a byte-stable no-op.
#[test]
#[ignore = "needs sync + export"]
fn sync_export_roundtrip_is_idempotent() {
    unimplemented!()
}

/// TODO(ws2/check): seeded vault with unknown property, bad relation range, and
/// dangling wiki link → all three reported with file, line, suggestion; nonzero.
#[test]
#[ignore = "needs check"]
fn check_reports_three_seeded_error_kinds() {
    unimplemented!()
}

/// TODO(ws2/sync): one file with malformed YAML is reported and skipped; the
/// rest of the vault still syncs.
#[test]
#[ignore = "needs sync"]
fn malformed_frontmatter_is_skipped_not_fatal() {
    unimplemented!()
}
