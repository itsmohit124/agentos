# AgentOS Evaluation System

Evaluation is a first-class subsystem beginning in Phase 0.

## Test layers

1. Unit tests
2. Integration tests
3. Fixture repository tests
4. Golden/retrieval tests
5. End-to-end workflow evaluations
6. Adapter compatibility tests
7. Regression tests for every discovered defect

## Fixture philosophy

Fixtures are small, realistic repositories with known ground truth. Include multiple technology stacks and deliberately bad architectures.

Recommended fixtures:
- `python-fastapi/`
- `node-express/`
- `typescript-react/`
- `go/`
- `java/`
- `ruby-rails/`
- `bad-architecture/`

## Golden case shape

```json
{
  "task": "Add retry support to payment webhook processing",
  "must_include": ["domain.payment.entity.Payment"],
  "must_not_include": ["fleet", "analytics"],
  "required_fragments": ["webhook_handler.py", "test_webhook_idempotency.py"]
}
```

Exact thresholds are derived from measured baselines, never invented.

## Core metrics

Knowledge: node precision/recall, edge precision/recall, provenance coverage.
DDD: context detection, terminology ambiguity, invariant mapping, lifecycle correctness.
Context: required-context recall, irrelevant-context rate, token count, reduction versus baseline, latency, reproducibility.
Skills: discovery, routing, dependency correctness, permission enforcement.
Feedback: validation detection and structured diagnostics.
Architecture: known-violation detection.
Workflow: execution order, ready frontier, resume behavior, bounded repair loops.
Memory: retrieval precision, staleness, forget semantics, secret redaction.
Adapters: install, idempotency, compatibility, uninstall safety.

## Evaluation traceability

Every context and major workflow evaluation should make it possible to answer:
- what inputs were used?
- what knowledge was selected?
- what was excluded and why?
- what tools/permissions were granted?
- what validation evidence was produced?

## Release gate

A release is green only when the relevant automated tests and evaluations pass, regression coverage remains green, and all phase gates are approved.

## Additions

**Golden case optional fields:** `max_tokens` and `max_irrelevant_ratio` are `null` until the Phase 4 baseline has been measured; only then are they set. Cases also record `fixture`, the deterministic oracle used, and (for context cases) `required_fragments`.

**Eval folders:** `evals/{knowledge,ddd,context,skills,workflow,feedback,architecture,memory,adapters,end-to-end,baselines}/`.

**Evaluate intermediate artifacts, not only final code:** project profile, knowledge graph, context pack, plan, permissions, feedback, final changes.

**Regression rule:** every bug becomes a permanent case (e.g. `evals/context/payment_webhook_007.json` for "Notifications context retrieved for a Payment task"). `agentos evaluate <area>` must keep it green.

**Dashboard (Phase 10):** `agentos evaluate` prints Knowledge / DDD / Context / Skills / Feedback / Adapters sections and an overall PASS/FAIL using REAL measured values; thresholds come from `evals/thresholds.json`, derived from baselines.

**Known-bad fixture:** `fixtures/bad-architecture/` (circular dependencies, fat controllers, DB calls in domain, duplicate terminology, broken invariants, shallow modules, missing tests) with ground truth.
