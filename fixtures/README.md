# Fixtures

Small, realistic repositories with known ground truth, used by tests and evaluations.

## Convention

```text
fixtures/<name>/
├── README.md        # what the fixture represents and what it is used for
├── ...              # the repository content itself
└── expected/        # ground truth consumed by tests/evaluations (never read by AgentOS itself)
```

- Fixtures are read-only inputs. Tests that need to modify a fixture copy it first with
  `agentos_testkit::copy_fixture("<name>")`.
- Ground truth is reviewed against the extraction rules before it is committed. `expected/` is listed
  in each fixture's `.agentosignore`, so a scan of the fixture does not index the oracle.

## Fixtures

| Name | Phase | Purpose |
|---|---|---|
| `placeholder` | 0 | Exercises the fixture loader only. No ground truth beyond its own identity. |
| `skills/` | 1 | Small skill trees for manifest parsing, discovery priority, duplicates, broken overrides, dependency cycles, missing dependencies, version constraints, implicit model invocation and broken built-ins. Ground truth lives in `evals/skills/*.json`; see `skills/README.md`. |
| `empty-project` | 2 | No content signals (README is ignored). Expected: `GREENFIELD` / `EMPTY`, uncertain. |
| `greenfield-specified` | 2 | Specs, docs and a manifest, no source. Expected: `GREENFIELD` / `SPECIFIED`. |
| `existing-project` | 2 | Source, tests, docs and a manifest, no spec. Expected: `EXISTING` / `ESTABLISHED`. |
| `hybrid-project` | 2 | Source and a specification. Expected: `HYBRID` / `ESTABLISHED`. |
| `python-fastapi` | 2 | Payments, users, notifications, tests, ADR, API, database, CI, container. Graph ground truth in `expected/graph.json`. |
