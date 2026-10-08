# AgentOS Knowledge Model

## Source of truth

The repository remains the source of truth. The knowledge index is derived state.

## Common node envelope

Every node should conceptually contain:

```text
id
node_type
name
scope
summary
status
provenance
confidence
version
fingerprint
source_refs
retrieval_metadata
payload
```

## Core node types

### Domain

- bounded_context
- entity
- value_object
- aggregate
- invariant
- domain_service
- policy
- domain_event
- terminology

### Software

- source_file
- symbol
- module
- interface
- api
- repository
- database_table
- external_service

### Engineering

- adr
- architecture_component
- constraint
- decision

### Quality

- test
- test_suite
- validation

### Agent

- skill
- tool
- capability

### Memory

- task
- episode
- observation
- lesson
- research

## Provenance

Every assertion should identify whether it is:

- observed: derived directly from repository/tool output
- declared: explicitly supplied by a human/project artifact
- inferred: model/system interpretation
- generated: derived artifact such as summaries or maps

Do not silently upgrade inferred information to canonical truth.

## Identity

Logical IDs should be stable across source-file moves. Example:

```text
domain.payment.entity.Payment
```

rather than using only a path-based identifier.

## Edge model

Edges should have:

```text
id
from
relation_type
to
confidence
provenance
metadata
```

Useful relations include:

### Domain

- BELONGS_TO
- CONTAINS
- HAS_ENTITY
- HAS_VALUE_OBJECT
- HAS_AGGREGATE
- CONSTRAINED_BY
- GOVERNS
- CROSS_CONTEXT_TO
- ANTI_CORRUPTS

### Software

- IMPLEMENTS
- EXPOSES
- CALLS
- DEPENDS_ON
- IMPORTS
- ADAPTS
- PROVIDES
- CONSUMES

### Quality

- TESTS
- VERIFIES
- VALIDATES
- VIOLATES

### Engineering

- GOVERNED_BY
- JUSTIFIED_BY
- SUPERSEDES
- CONFLICTS_WITH
- DERIVED_FROM

### Agent

- REQUIRES
- PRODUCES
- CONSUMES
- USES_TOOL
- INVOKES

## Fragment model

Fragments are the units that can be included in a Context Pack.

A fragment should contain:

```text
id
node_id
source_ref
location
content
fingerprint
token_estimate
kind
```

Examples:
- a class body
- a function
- a test case
- an ADR section
- a terminology entry
- an architecture rule

## Why fragments matter

A 900-line source file can describe one knowledge node but expose only a 20-line method as the relevant evidence. Nodes describe structure; fragments control context cost.
