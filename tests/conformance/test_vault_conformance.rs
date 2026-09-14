//! Vault-LD conformance harness (WS2 deliverable 5).
//!
//! Parse-slice tests run against MIT-safe seeded vaults under `vaults/`.
//! Sync/export/isomorphism tests stay `#[ignore]`d until those slices land.
//! See `tests/conformance/README.md` for fixture sourcing and the plan.

use std::path::PathBuf;

use sparrowdb_vault::{check, DiagnosticKind, Vault};

/// Upstream commit the conformance fixtures and reference exporter are pinned to.
const VAULT_LD_UPSTREAM_REV: &str = "025e71be8d810e387dc451c920e05472d1c47170";

fn seeded_vault(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/conformance/vaults")
        .join(name)
}

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

/// Seeded vault with unknown property, bad relation range, and dangling wiki
/// link → all three reported with file, line, suggestion; nonzero.
/// UnknownProperty / RelationRangeViolation need the ontology (sync slice).
#[test]
#[ignore = "needs UnknownProperty and RelationRangeViolation from the sync slice"]
fn check_reports_three_seeded_error_kinds() {
    unimplemented!()
}

#[test]
fn clean_vault_check_is_clean() {
    let vault = Vault::open(seeded_vault("clean")).unwrap();
    let report = check(&vault).unwrap();
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    assert!(report.files_checked >= 2);
}

#[test]
fn check_reports_dangling_wiki_link() {
    let vault = Vault::open(seeded_vault("dangling-link")).unwrap();
    let report = check(&vault).unwrap();
    assert!(report
        .errors
        .iter()
        .any(|d| d.kind == DiagnosticKind::DanglingWikiLink));
    assert!(report.errors.iter().all(|d| d.suggestion.is_some()));
}

#[test]
fn check_reports_relative_explicit_id() {
    let vault = Vault::open(seeded_vault("relative-id")).unwrap();
    let report = check(&vault).unwrap();
    assert!(report
        .errors
        .iter()
        .any(|d| d.kind == DiagnosticKind::RelativeExplicitId));
}

/// One file with malformed YAML is reported and skipped; the rest of the vault
/// is still checked (sync will reuse this skip-not-fatal behaviour).
#[test]
fn malformed_frontmatter_is_skipped_not_fatal() {
    let vault = Vault::open(seeded_vault("malformed-yaml")).unwrap();
    let report = check(&vault).unwrap();
    assert!(
        report.files_checked >= 2,
        "other notes must still be visited, checked {}",
        report.files_checked
    );
    let malformed: Vec<_> = report
        .errors
        .iter()
        .filter(|d| d.kind == DiagnosticKind::MalformedFrontmatter)
        .collect();
    assert_eq!(malformed.len(), 1);
    assert!(malformed[0].file.ends_with("broken.md"));
    assert!(malformed[0].line.is_some());
    assert!(report
        .errors
        .iter()
        .all(|d| d.kind == DiagnosticKind::MalformedFrontmatter));
}
