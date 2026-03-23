# Personal Ontology

Durable AI memory that persists across conversations.

---

## What this is for

Every conversation with an AI agent starts blank. By the end of the session it knows your name, your projects, your preferences — and then it forgets everything. The next session starts blank again.

A personal ontology is a persistent graph that the agent reads at the start of every session. It stores the things worth remembering: relationships, decisions, beliefs, projects, events, concepts. The agent picks up where it left off.

---

## What to capture

**Capture:**
- People you know and how you know them
- Projects you're working on and their status
- Decisions you've made and why
- Events that happened (or will happen) and who was involved
- Concepts you think about and how they connect
- Places that matter and their relationships to people/events

**Don't capture:**
- Ephemeral tasks ("send email to Alice today")
- Drafts and work-in-progress that will be superseded
- Information that changes faster than you'll query it
- Noise — things you noted but never need to retrieve

The test: "Would I want an agent to know this two years from now?" If yes, it belongs in the graph. If no, it doesn't.

---

## Start with the PersonalKnowledge template

```
start_here({ "template": "PersonalKnowledge" })
```

This seeds five classes purpose-built for personal use:
- `Person` — people you know
- `Concept` — ideas, domains, topics you think about
- `Event` — things that happened or will happen
- `Location` — places that matter
- `Document` — notes, articles, books, conversations

Five classes is enough to start. Resist adding more until you have 20+ entities and natural clusters emerge that need their own class.

---

## Key relations

| Relation | Domain → Range | Use for |
|----------|---------------|---------|
| `KNOWS` | Person → Person | People who know each other |
| `RELATED_TO` | Concept → Concept | Concepts that connect |
| `PARTICIPATED_IN` | Person → Event | Who was at what |
| `OCCURRED_AT` | Event → Location | Where things happened |
| `LOCATED_IN` | Person/Org → Location | Where people/orgs are based |
| `AUTHORED` | Person → Document | Who wrote what |
| `CITES` | Document → Document | Reference chains |

---

## Seeding strategy

Start from sources you already have:

1. **Contact list** — import key people with name and relationship context
2. **Calendar/Granola transcripts** — events you participated in, decisions made
3. **Notes / journal** — concepts you've been thinking about
4. **Projects** — active work with dependencies and owners

Don't try to import everything. Import the 20 things you'd want an agent to know about you on day one.

---

## Example session

```
// Start of session
health()          → {"status": "ok"}
start_here()      → {initialized: true, class_count: 5}

// Agent loads context
find_entities("Person", {})          → [Alice, Bob, Carol, ...]
find_entities("Project", {})         → [SparrowOntology, ...]

// Agent writes new information from the session
create_entity("Event", {
  "name": "Arch review with Alice",
  "date": "2026-03-23"
})
create_relationship(alice_id, event_id, "PARTICIPATED_IN")
```

The graph accumulates. Every session adds to it. Over time the agent has richer context than any single conversation could build.

---

## Design principles

- **Classes are nouns.** `Person`, `Event`, `Concept` — not `PersonKnowledgeItem`.
- **Don't create a class for what should be a property.** "Alice is a founder" → `Person.role = "founder"`. Not a `Founder` class.
- **If you can't name the relationship, you don't understand the connection.** `RELATED_TO` is a placeholder. Replace it with something specific when you know what the relationship actually is.
- **5 classes, not 20.** Every class you add is one more thing to keep seeded and consistent. Start small.
