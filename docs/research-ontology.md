# Research Ontology

Claims, evidence, and provenance as first-class graph citizens.

---

## Why research needs different schema

Standard knowledge graphs track facts. Research graphs track **claims** — assertions that may be supported, refuted, uncertain, or contested. The difference matters:

- A fact: `Person.name = "Alice"`
- A claim: "Alice's methodology increases throughput by 40% — based on one study, confidence 0.6"

A claim without `source` and `confidence` is just an assertion you can't evaluate. Every claim in a research graph must carry its provenance.

---

## Start with ResearchNotes

```
start_here({ "template": "ResearchNotes" })
```

Seeds five classes:
- `Claim` — the core unit. Every insight, finding, hypothesis, or assertion.
- `Document` — papers, articles, transcripts, notes that contain claims
- `Concept` — ideas, theories, domains, named abstractions
- `Person` — authors, researchers, sources
- `Asset` — datasets, codebases, outputs that claims are about

---

## Claim is the core unit

Everything else supports claims. A `Document` exists because it contains claims. A `Person` exists because they authored or contested a claim. A `Concept` exists because claims are about it.

Required properties on every `Claim`:
```
add_property(owner="Claim", name="statement", datatype="string", required=true)
add_property(owner="Claim", name="source", datatype="string", required=true)
add_property(owner="Claim", name="confidence", datatype="float", required=true)
```

`confidence` is a float from 0.0 (speculation) to 1.0 (verified). Use it consistently — 0.9+ means well-supported, 0.5–0.8 means plausible, below 0.5 means worth noting but not acting on.

---

## Key relations for research

| Relation | Domain → Range | Use for |
|----------|---------------|---------|
| `CITES` | Document → Document | Citation chain |
| `SUPPORTS` | Claim/Document → Claim | Evidence for a claim |
| `CONTRADICTS` | Claim/Document → Claim | Evidence against |
| `DERIVED_FROM` | Claim/Asset → Claim/Asset | Lineage — derived findings |
| `AUTHORED` | Person → Document/Claim | Who wrote/made it |
| `TAGGED_WITH` | Claim/Document → Concept | Conceptual classification |

---

## Example: a contested claim

```
// Create the claim
create_entity("Claim", {
  "statement": "LLM agents with structured memory outperform baseline on long-horizon tasks",
  "source": "Smith et al. 2024, arXiv:2401.XXXXX",
  "confidence": 0.72
})  // → claim_id: "100"

// Create supporting evidence
create_entity("Document", {
  "title": "Smith et al. 2024",
  "url": "https://arxiv.org/abs/2401.XXXXX"
})  // → doc_id: "101"

create_relationship(doc_id="101", claim_id="100", "SUPPORTS")

// Create a contradicting claim
create_entity("Claim", {
  "statement": "Performance gains from structured memory are within noise margins",
  "source": "Jones 2024, replication study",
  "confidence": 0.55
})  // → counter_id: "102"

create_relationship(counter_id="102", claim_id="100", "CONTRADICTS")
```

Now you can query: "which claims contradict my main hypothesis?" — and each result carries the source and confidence of the contradiction.

---

## Provenance chains

`DERIVED_FROM` tracks how findings build on each other:

```
Claim: "Agents need structured memory" (confidence: 0.9)
  ← DERIVED_FROM ← Claim: "Baseline agents forget context" (confidence: 0.95)
  ← DERIVED_FROM ← Claim: "Context loss correlates with task failure" (confidence: 0.85)
```

When a foundational claim's confidence drops (new replication fails), you can traverse the graph to find all derived claims that need re-evaluation.

---

## Design principles for research graphs

- **Every Claim needs `source` and `confidence`.** No exceptions. An unsourced claim is useless for research.
- **`SUPPORTS` and `CONTRADICTS` are the highest-value relations.** Invest time in wiring them. They're what makes the graph useful for synthesis, not just storage.
- **Don't conflate Claim and Document.** A Document contains Claims; it is not itself a Claim. A paper that argues X produces a Claim node for X, linked to the Document node via `AUTHORED` + `SUPPORTS`.
- **Confidence degrades through DERIVED_FROM chains.** A claim derived from a 0.6-confidence finding should itself be no higher than 0.6, probably lower.
- **Use `TAGGED_WITH` for thematic navigation.** Tag claims with `Concept` nodes for the research domains they belong to. This makes "find all claims about X" fast.
