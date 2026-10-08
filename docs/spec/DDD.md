# Domain-Driven Design Contract

AgentOS treats DDD knowledge as explicit project knowledge, not unquestioned LLM output.

## Strategic design

Represent:
- bounded contexts
- ubiquitous language
- context map
- anti-corruption layers
- upstream/downstream relationships
- integration direction

## Tactical design

Represent:
- entities
- value objects
- aggregates and aggregate roots
- invariants
- domain services
- repositories
- policies
- domain events

## Truth lifecycle

Domain assertions use:

`Observed -> Inferred -> Proposed -> Confirmed -> Canonical`

Inference may create `Inferred` or `Proposed` knowledge only. It can never silently create `Confirmed` or `Canonical` knowledge.

## Ubiquitous language

`.agentos/knowledge/domain/ubiquitous-language.md` is the human-readable canonical terminology artifact. Conflicts and synonyms are findings, not automatic merges.

## Aggregate discipline

Aggregates define transactional boundaries and protect business invariants. AgentOS should flag direct mutation or cross-aggregate logic when evidence shows the existing architecture enforces aggregate boundaries.

## Hexagonal architecture

When applicable, AgentOS models the domain core separately from ports and adapters:

`Domain -> Ports -> Adapters`

Infrastructure must not leak into the domain without an explicit, evidenced design decision.

## Corrective policies

Where invariants cannot be enforced synchronously, a corrective policy may periodically detect, flag, or repair inconsistent state. Such policies must be explicit project knowledge and carry provenance.
