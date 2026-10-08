# AgentOS Context Engine

## Goal

Return the smallest context set that preserves the information required to perform the task safely and correctly.

## Never use repository-wide dumping as the default retrieval strategy.

## Retrieval pipeline

```text
User request
  -> intent extraction
  -> domain routing
  -> graph candidate retrieval
  -> structural expansion
  -> semantic/lexical retrieval
  -> relevance ranking
  -> dependency / constraint enrichment
  -> token budgeting
  -> Context Pack
```

## Context Pack

A Context Pack contains:

```text
task
intent
budget
selected_nodes
selected_fragments
terminology
architecture
constraints
invariants
decisions
tests
exclusions
retrieval_trace
```

## Selection principles

Priority order:

1. mandatory task constraints
2. directly relevant domain concepts
3. directly relevant architecture
4. explicit decisions/ADRs
5. relevant invariants and tests
6. directly relevant code fragments
7. supporting context
8. historical/background information

Remove low-priority context first when over budget.

## Retrieval methods

Use multiple complementary methods:

- exact/lexical search
- structural graph traversal
- symbol dependency expansion
- semantic retrieval when enabled

Vector similarity is a retrieval aid, not the system's source of truth.

## Explainability

Every Context Pack should include a retrieval trace sufficient to answer:
- why was this item selected?
- why was this item excluded?
- what relationship caused expansion?
- what source supports the assertion?
- what was the token estimate?

## Context caching

Persist successful task-to-context patterns. Reuse them only when the underlying knowledge fingerprints have not changed.

## Evaluation metrics

At minimum:

- required-context recall
- irrelevant-context rate
- token count
- token reduction versus baseline
- retrieval latency
- reproducibility

Do not optimize token count at the expense of required knowledge recall.
