# Documentation Workplan

Linear free tier limit hit. This file is the source of truth for pending doc work.
Linear ticket SPA-272 covers the README update. Everything else is tracked here.

---

## Priority order

1. Skill file update (highest leverage — every AI agent session)
2. README (SPA-272 — first impression on crates.io/GitHub)
3. docs/agent-world-model.md (highest-value use case)
4. docs/personal-ontology.md (entry point)
5. docs/team-ontology.md
6. docs/mcp-reference.md
7. docs/schema-reference.md
8. docs/research-ontology.md
9. Cargo.toml metadata (repository/homepage/docs URLs) + 0.1.1 publish

---

## 1. Skill file update

File: `skills/sparrow-ontology.skill`

Missing from current skill:
- `health` tool (SPA-269) — operational ping, preflight before sessions
- `stats` tool (SPA-270) — schema analytics + entity counts
- Four starter templates and when to choose each:
  - `WorldModel` — general purpose, 10 classes
  - `PersonalKnowledge` — Person, Concept, Event, Location, Document
  - `ProfessionalNetwork` — Person, Organization, Role, Project, Event
  - `ResearchNotes` — Concept, Document, Claim, Person, Asset
- Property inheritance: subclass entities (e.g. Employee < Person) require
  parent class required properties (e.g. Person.name)
- Review full preflight checklist against current 17-tool surface

---

## 2. README (SPA-272)

- crates.io badges for all three crates
- Replace git clone install with `cargo add` / `cargo install`
- Add `health` and `stats` to MCP tool table
- Mention all four starter templates
- Update test count: 86 → 117
- Link to /docs once populated

---

## 3. docs/agent-world-model.md

The high-value use case: structured shared context for multi-agent coordination.

Key content:
- Why schema matters for agents: without it, agents drift (agent A writes "Person",
  agent B writes "person", queries break)
- Schema is the contract between agents — define before deploying
- Relations encode authority — which agent is the authoritative writer per class?
- `health` and `stats` are agent-native monitoring tools
- Aliases absorb spelling drift across model versions and prompts
- `validate` regularly — agents make mistakes
- Seeding strategy: define schema from task domain, not from data you already have
- Template: `ProfessionalNetwork` or `WorldModel`

---

## 4. docs/personal-ontology.md

Entry point. One person, no coordination overhead.

Key content:
- For: durable AI memory across conversations, personal knowledge graph
- Capture: relationships, beliefs, decisions, projects, events, concepts
- Don't capture: ephemeral tasks, drafts, noise
- Design: `PersonalKnowledge` template, 5-7 classes max initially
- Seeding: start from memory/Granola transcripts, journal entries, contact list
- Key relations: KNOWS, RELATED_TO, PARTICIPATED_IN, OCCURRED_AT, LOCATED_IN

---

## 5. docs/team-ontology.md

Shared contract between humans and agents on a team.

Key content:
- Critical difference from personal: **authority matters** — ownership must be unambiguous
- Capture: org structure, roles, projects, assets, decisions, accountability
- Before populating: agree on canonical class/relation names
- Common failure: too many classes before relations are wired
- Template: `ProfessionalNetwork`
- Key relations: WORKS_FOR, MEMBER_OF, LEADS, OWNS, DEPENDS_ON, HAS_ROLE

---

## 6. docs/mcp-reference.md

All 17 tools with parameters and example responses:
start_here, get_ontology, define_class, define_relation, add_property, add_alias,
define_subclass, define_subproperty, resolve_name, validate, create_entity,
create_relationship, update_entity, find_entities, explain_symbol, health, stats

---

## 7. docs/schema-reference.md

Pre-seeded WorldModel schema:
- 10 classes with all properties
- 19 relations with domain/range constraints
- 22 properties with types and required flags
- Built-in aliases

---

## 8. docs/research-ontology.md

Claims, evidence, provenance as first-class.

Key content:
- Every Claim needs `source` and `confidence`
- Template: `ResearchNotes`
- Claim nodes are the core unit — everything supports them
- Key relations: CITES, SUPPORTS, CONTRADICTS, DERIVED_FROM, AUTHORED

---

## 9. Cargo.toml metadata

Add to all three crates `[package]`:
```toml
repository = "https://github.com/ryaker/SparrowOntology"
homepage = "https://github.com/ryaker/SparrowOntology"
documentation = "https://docs.rs/sparrowdb-ontology-core"  # adjust per crate
```
Bump to 0.1.1 and republish after docs are stable.

---

## Cross-cutting design principles (weave into each domain doc)

- Classes are nouns, relations are verbs
- Don't create a class for what should be a property
- If you can't name the relationship, you don't understand the connection
- Start with 5 classes, not 20
- Add subclasses when 10+ instances naturally split
- Relations are harder to get right than classes — spend time on them
