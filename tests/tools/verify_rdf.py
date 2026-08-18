#!/usr/bin/env python3
"""Independent RDF verification for the WS1 instance-data exporter.

This is a **test-only** script. Nothing in the shipped crates depends on it, and
it adds no runtime dependency — it exists so the Turtle and JSON-LD that
`sparrowdb-ontology-core` emits are checked by a third-party RDF implementation
(rdflib) rather than only by the same oxrdf library that produced them.

Usage:
    verify_rdf.py <export1.ttl> <export2.ttl> <export1.jsonld>

Checks:
  1. Both Turtle files parse cleanly.
  2. The JSON-LD parses cleanly through rdflib's JSON-LD 1.1 processor.
  3. export1 and export2 are graph-isomorphic (the round-trip guarantee).
  4. The JSON-LD serialization is isomorphic to the Turtle serialization
     (the two exports describe one graph, not two).

Exits 0 on success, 1 on failure, and 77 (skip) if rdflib is unavailable so the
Rust harness can distinguish "not checked" from "checked and wrong".
"""

import sys

try:
    from rdflib import Graph
    from rdflib.compare import isomorphic, graph_diff
except ImportError:
    print("SKIP: rdflib not installed", file=sys.stderr)
    sys.exit(77)


def load(path, fmt):
    g = Graph()
    g.parse(path, format=fmt)
    return g


def report_diff(label, a, b):
    _, only_a, only_b = graph_diff(a, b)
    print(f"FAIL: {label} are not isomorphic", file=sys.stderr)
    for t in sorted(only_a, key=str)[:20]:
        print(f"  only in first:  {t}", file=sys.stderr)
    for t in sorted(only_b, key=str)[:20]:
        print(f"  only in second: {t}", file=sys.stderr)


def main():
    if len(sys.argv) != 4:
        print(__doc__, file=sys.stderr)
        return 1
    ttl1, ttl2, jsonld1 = sys.argv[1:4]

    g1 = load(ttl1, "turtle")
    g2 = load(ttl2, "turtle")
    gj = load(jsonld1, "json-ld")

    print(f"parsed: {ttl1}={len(g1)} triples, {ttl2}={len(g2)} triples, "
          f"{jsonld1}={len(gj)} triples")

    # The exporter never emits blank nodes (WS1 non-goal), so for these ground
    # graphs isomorphism coincides with set equality. isomorphic() is used
    # anyway: it is the check the spec names, and it stays correct if blank
    # nodes are ever introduced.
    ok = True
    if not isomorphic(g1, g2):
        report_diff("the two Turtle exports", g1, g2)
        ok = False
    if not isomorphic(g1, gj):
        report_diff("Turtle and JSON-LD", g1, gj)
        ok = False

    if ok:
        print("OK: exports are graph-isomorphic (Turtle round-trip and JSON-LD)")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
