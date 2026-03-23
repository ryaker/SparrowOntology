# Team Ontology

A shared schema contract between humans and agents on a team.

---

## The critical difference from personal use

A personal ontology is yours alone. You can rename classes, refactor properties, and start over without affecting anyone.

A team ontology is a **shared contract**. Every agent and every human who writes to the graph is bound by it. Once `Person.name` exists and 50 people have been created against it, renaming it to `full_name` is a migration. Changing the domain of `LEADS` requires updating every existing relationship.

**Authority must be unambiguous.** Before you populate the graph, agree on:
1. Canonical class names and property names
2. Which team/agent owns writes to each class
3. Who can modify the schema (add new classes, relations, properties)

If you skip this step, you'll end up with fragmentation — multiple agents writing their own variants of the same concept.

---

## Start with ProfessionalNetwork

```
start_here({ "template": "ProfessionalNetwork" })
```

Seeds: `Person`, `Organization`, `Role`, `Project`, `Event`

This covers most team knowledge graph needs out of the box. The classes are generic enough to extend but specific enough to anchor org structure and project relationships.

---

## Key relations for team use

| Relation | Domain → Range | Use for |
|----------|---------------|---------|
| `WORKS_FOR` | Person → Organization | Employment |
| `MEMBER_OF` | Person → Organization/Project | Team membership |
| `LEADS` | Person → Project/Team | Ownership/leadership |
| `OWNS` | Person/Org → Asset/Project | Accountability |
| `DEPENDS_ON` | Project → Project | Dependency tracking |
| `HAS_ROLE` | Person → Role | Role assignments |

---

## What to capture

**Capture:**
- Org structure — who works for what, who leads what
- Projects and their dependencies
- Roles and who holds them (with dates)
- Decisions made at the team level, and who made them
- Assets that teams produce and own

**Common failure:** adding too many classes before relations are wired.

You don't need a `Decision` class and a `Conclusion` class and a `Choice` class. Pick one and make it work. Add the second when the first has 20+ entities and you can articulate exactly why they differ.

---

## Before populating

Run this checklist before any agent starts writing:

1. `health()` — service is up
2. `get_ontology()` — all canonical class/relation names are agreed
3. Aliases defined for all variants teams might use:
   ```
   add_alias("emp", "Person", "class")
   add_alias("org", "Organization", "class")
   add_alias("proj", "Project", "class")
   ```
4. Required properties declared on all classes agents will write
5. Write authority documented — each class has exactly one authoritative writer

---

## Subclass example: org hierarchy

```
define_class("Person")
add_property(owner="Person", name="name", required=true)

define_class("Employee")
define_subclass(child="Employee", parent="Person")
add_property(owner="Employee", name="employee_id", required=true)

define_class("Manager")
define_subclass(child="Manager", parent="Employee")
add_property(owner="Manager", name="department", required=true)
```

`Manager` entities require `name` (Person), `employee_id` (Employee), `department` (Manager). The inheritance chain is validated automatically — no duplicating parent properties on child classes.

---

## Design principles (applied to teams)

- **Agree on names before you have data.** Renaming after the fact is a migration.
- **Relations are harder to get right than classes.** Spend the time. A wrong relation name is worse than a missing one — missing ones you can add, wrong ones corrupt existing data.
- **Authority matters.** If two agents can both write `Project.status`, you'll get race conditions and conflicting state. One writer per class.
- **Validate continuously.** Run `validate` before bulk writes. Run `stats` to monitor for unexpected growth patterns.
- **Add subclasses when 10+ instances naturally split.** Not before.
