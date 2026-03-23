# Agent World Model

Schema-enforced shared memory for multi-agent systems.

---

## The problem agents create

Deploy two agents against the same graph without a schema contract and this happens within a week:

```
Agent A writes:  Person { name: "Alice", role: "engineer" }
Agent B writes:  person { Name: "Bob",   title: "engineer" }
Agent C writes:  Employee { full_name: "Carol", job: "eng" }
```

Now find all engineers. The query `MATCH (p:Person)` misses half the data. Every agent that comes after has to guess which variant is canonical. Retrieval degrades. Context becomes noise.

**This isn't a model quality problem. It's a missing contract problem.**

---

## Schema is the contract between agents

Before deploying any agent that writes to a shared graph:

1. Define the schema — classes, relations, properties, subclasses
2. Define ownership — which agent is the authoritative writer per class
3. Publish the schema to all agents via `start_here` / `get_ontology`
4. Validate continuously — `validate` before writes, `health` before sessions

The schema enforces itself. An agent can't write `Employee.full_name` if `full_name` isn't declared on `Employee`. It gets an error that says exactly what to call to fix it.

---

## Seeding strategy

**Start from your task domain, not from data you already have.**

If you're building a project coordination system, your initial classes are not "whatever exists in our spreadsheet." They are the concepts that the task actually manipulates:

- Who does work? → `Person`
- What gets built? → `Project`, `Task`
- Who owns what? → Relations: `OWNS`, `LEADS`, `ASSIGNED_TO`

Use the `ProfessionalNetwork` template as a starting point:

```
start_here({ "template": "ProfessionalNetwork" })
```

This seeds: `Person`, `Organization`, `Role`, `Project`, `Event` with a base set of relations. Extend from there — don't start from scratch unless your domain is genuinely different.

**Rule:** 5 classes, not 20. Add classes when 10+ entities of a type naturally emerge and need their own properties. Adding classes too early just creates unseeded noise.

---

## Relations encode authority

Relations aren't just traversal edges — they encode which agents have write authority over which class connections.

Example: if only the `ProjectManager` agent should create `LEADS` relationships, make that an operational rule:
- `ProjectManager` agent: writes `Person`, `Project`, `LEADS`
- `TaskAgent` agent: reads `Person`, `Project`; writes `Task`, `ASSIGNED_TO`
- `EventAgent` agent: reads `Person`; writes `Event`, `PARTICIPATED_IN`

Use `define_relation` with strict `domain`/`range` constraints. An agent trying to link `Task LEADS Project` will get a domain violation at write time — the constraint catches it without requiring the agents to coordinate at runtime.

---

## Health and stats as agent-native monitoring

Before any write session, call `health`:

```
→ health()
{ "status": "ok", "db_path": "/Users/ryaker/sparrow-ontology.db" }
```

If health fails, stop. Do not attempt writes against a degraded service. Report the error and wait for restart.

Call `stats` to understand the current state of the graph before making decisions:

```
→ stats()
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

Use `stats` to detect drift: if `Task` count is growing but `Project` count is flat, something's wrong with project creation. Build `stats` into your agent's preflight checklist.

---

## Aliases absorb spelling drift

Model versions drift. One Claude session writes `Organization`, another writes `org`, another writes `company`. Without aliases, these create three separate classes.

Register aliases before deploying:

```
add_alias("org", "Organization", "class")
add_alias("company", "Organization", "class")
add_alias("co", "Organization", "class")
```

Now all three spellings resolve to `Organization` at write time. No data fragmentation. No cleanup pass. The alias registry is persistent — define it once, it applies to every future agent session.

---

## Property inheritance

Subclass entities inherit required properties from their ancestors.

```
Person (required: name)
  └── Employee (required: employee_id)
        └── Manager (required: department)
```

A `Manager` entity must supply `name` (from `Person`), `employee_id` (from `Employee`), and `department` (from `Manager`). The validation walks the entire ancestry chain — you don't need to redeclare parent properties on child classes.

Before writing subclass entities, call `explain_symbol("Manager")` to see the full property inheritance chain.

---

## Validate regularly

Agents make mistakes. Schema constraints catch mistakes at write time, but agents can also:
- Read a stale `get_ontology` snapshot and use wrong property names
- Try to create relationships with wrong domain/range
- Accumulate orphaned entities when a multi-step write fails partway

Run `validate` before any multi-step write sequence:

```
→ validate("Person", { "name": "Alice", "role": "engineer" })
Error: Unknown property 'role'. Valid: ["name", "email", "location"].
Call add_property(owner='Person', name='role') to declare it first.
```

`validate` is a dry run — it checks the schema and returns the same errors as `create_entity`, without writing anything. Use it in agent planning phases before committing to a write sequence.

---

## Recommended WorldModel template for multi-agent coordination

The `WorldModel` template (default) seeds 10 classes and 19 relations purpose-built for general-purpose agentic tasks:

**Classes:** `Person`, `Organization`, `Project`, `Document`, `Event`, `Location`, `Concept`, `Asset`, `Role`, `Claim`

**Key relations for coordination:** `OWNS`, `LEADS`, `DEPENDS_ON`, `MEMBER_OF`, `HAS_ROLE`, `PARTICIPATED_IN`

Use `WorldModel` when your agent system spans multiple domains and you need a general-purpose shared vocabulary. Use `ProfessionalNetwork` when the primary domain is org structure and project execution.

---

## Checklist: before deploying multi-agent writes

- [ ] `health()` returns `"status": "ok"`
- [ ] `get_ontology()` reviewed — all required classes and relations exist
- [ ] Property seeding verified — `start_here` shows no unexpected `unseeded_classes`
- [ ] Aliases defined for all known spelling variants
- [ ] Write authority assigned per class/relation — no two agents own the same class
- [ ] `validate` tested with representative payloads before going live
- [ ] `stats` baseline recorded — monitor for unexpected entity count patterns
