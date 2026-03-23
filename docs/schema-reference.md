# Schema Reference — WorldModel

Pre-seeded schema from `init()` / `start_here({ "template": "WorldModel" })`.

10 classes · 19 relations · 22 properties · built-in aliases

---

## Classes

| Class | Description |
|-------|-------------|
| `Person` | A human individual |
| `Organization` | A company, team, institution, or group |
| `Project` | A bounded unit of work with goals and participants |
| `Document` | A written artifact: note, article, report, transcript |
| `Event` | A happening with participants, date, and location |
| `Location` | A physical or virtual place |
| `Concept` | An idea, topic, domain, or named abstraction |
| `Asset` | A produced artifact: code, design, dataset, output |
| `Role` | A function or title held by a person in an org/project |
| `Claim` | An assertion with source, confidence, and supporting evidence |

---

## Relations

All relations are directional: `(domain) -[RELATION]-> (range)`.

| Relation | Domain | Range | Meaning |
|----------|--------|-------|---------|
| `KNOWS` | Person | Person | Personal or professional connection |
| `WORKS_FOR` | Person | Organization | Employment or contracting |
| `MEMBER_OF` | Person | Organization/Project | Team or group membership |
| `LEADS` | Person | Project/Organization | Leadership or ownership |
| `AUTHORED` | Person | Document | Authorship of a written artifact |
| `OWNS` | Person/Org | Asset/Project | Ownership or accountability |
| `DEPENDS_ON` | Project | Project | Inter-project dependency |
| `CITES` | Document | Document | Citation or reference |
| `TAGGED_WITH` | any | Concept | Conceptual classification |
| `HAS_ROLE` | Person | Role | Role assignment |
| `PRODUCED` | Project/Person | Asset | Output of work |
| `PARTICIPATED_IN` | Person | Event | Event participation |
| `LOCATED_IN` | Person/Org | Location | Physical base or address |
| `OCCURRED_AT` | Event | Location | Where an event happened |
| `SUPPORTS` | Claim/Document | Claim | Evidential support |
| `CONTRADICTS` | Claim/Document | Claim | Evidential contradiction |
| `RELATED_TO` | any | any | Generic relationship (prefer specific relations) |
| `PART_OF` | any | any | Compositional containment |
| `DERIVED_FROM` | Asset/Claim | Asset/Claim | Derivation or lineage |

---

## Properties

Properties are declared on specific classes. `required: true` means `create_entity` will reject the entity if the property is missing.

| Property | Owner Class | Type | Required | Notes |
|----------|-------------|------|----------|-------|
| `name` | Person, Organization, Project, Location, Concept, Role | string | true | Primary identifier |
| `description` | all | string | false | Human-readable summary |
| `email` | Person | string | false | Contact email |
| `phone` | Person | string | false | Contact phone |
| `url` | Organization, Document, Asset | string | false | Web URL |
| `source` | Claim, Document | string | false | Origin or provenance |
| `confidence` | Claim | float | false | 0.0–1.0 confidence score |
| `start_date` | Event, Project, Role | string | false | ISO 8601 date |
| `end_date` | Event, Project, Role | string | false | ISO 8601 date |
| `created_at` | all | string | false | Record creation timestamp |
| `updated_at` | all | string | false | Last update timestamp |
| `location` | Person | string | false | Location string (use `LOCATED_IN` for graph traversal) |
| `education` | Person | string | false | Educational background |
| `status` | Project, Task | string | false | Current status |
| `priority` | Task | string | false | Priority level |
| `due_date` | Task | string | false | ISO 8601 due date |
| `title` | Document, Role | string | false | Title or heading |
| `content` | Document | string | false | Body text |
| `type` | Asset, Event | string | false | Subtype discriminator |
| `version` | Asset | string | false | Version string |
| `format` | Asset, Document | string | false | File format or media type |
| `size` | Asset | integer | false | Size in bytes |

---

## Built-in Aliases

Registered at init. Case-insensitive resolution is automatic for exact case matches — aliases are for common abbreviations and alternate spellings.

| Alias | Resolves to | Kind |
|-------|------------|------|
| `person` | `Person` | class |
| `org` | `Organization` | class |
| `organization` | `Organization` | class |
| `company` | `Organization` | class |
| `project` | `Project` | class |
| `doc` | `Document` | class |
| `document` | `Document` | class |
| `event` | `Event` | class |
| `location` | `Location` | class |
| `concept` | `Concept` | class |
| `asset` | `Asset` | class |
| `role` | `Role` | class |
| `claim` | `Claim` | class |

---

## Extending the schema

Add properties:
```
add_property(owner="Person", name="github_handle", datatype="string")
```

Add a class:
```
define_class("Team")
add_property(owner="Team", name="name", required=true)
define_relation("MEMBER_OF", "Person", "Team")
```

Add a subclass:
```
define_class("Engineer")
define_subclass(child="Engineer", parent="Person")
add_property(owner="Engineer", name="stack", datatype="string")
// Engineer.name is inherited from Person (required)
```

Register aliases for your team's vocabulary:
```
add_alias("eng", "Engineer", "class")
add_alias("reports_to", "LEADS", "relation")
```
