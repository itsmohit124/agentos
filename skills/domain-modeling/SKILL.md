---
name: domain-modeling
description: Build or sharpen the domain model - ubiquitous language, bounded contexts, aggregates and invariants - and record it as reviewable markdown.
version: "0.1.0"
invocation: both
inputs:
  - name: domain
    description: The business area to model and the question that motivates it.
  - name: sources
    description: Code, docs or conversations to draw terms and rules from.
    required: false
outputs:
  - name: glossary
    description: Proposed glossary entries (term, definition, context, source).
  - name: decisions
    description: ADR drafts for modelling decisions that need human confirmation.
    required: false
permissions: [fs.read, fs.write]
references: [references/glossary-template.md]
optional_recommends: [grilling]
triggers:
  phrases: [ubiquitous language, bounded context, domain terms, glossary]
  keywords: [domain, glossary, terminology, aggregate, invariant, entity]
  intents: [domain]
eval_cases:
  - task: Clarify the domain terminology for orders and invoices
  - task: Which invariants must the subscription aggregate protect
---
# Domain modeling

## When to use
Terms are ambiguous, a concept means different things in different places, or a change touches
business rules (invariants, lifecycles) that are not written down.

## Inputs
- The domain area and the motivating question.
- Optional sources: code, docs, tickets, conversation notes.

## Procedure
1. Collect candidate terms with where each was found.
2. Group terms by bounded context; flag terms used with different meanings across contexts.
3. For each aggregate, state its invariants and the lifecycle states it moves through.
4. Draft glossary entries using `references/glossary-template.md`.
5. Mark every entry as proposed and list the questions a domain expert must confirm.

## Outputs
Proposed glossary entries and, where a modelling choice was made, ADR drafts. All marked
proposed until a human confirms them.

## Must not
- Mark inferred terms or rules as confirmed or canonical.
- Rename code identifiers.
- Merge two contexts' meanings of a term without flagging the conflict.

## Validation
Every entry names its source and context, and every ambiguity found in step 2 appears as an
open question.
