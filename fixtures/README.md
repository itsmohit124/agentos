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
- Ground truth is hand-written and reviewed; it is never generated from AgentOS output.

## Fixtures

| Name | Phase | Purpose |
|---|---|---|
| `placeholder` | 0 | Exercises the fixture loader only. No ground truth beyond its own identity. |
| `skills/` | 1 | Small skill trees for manifest parsing, discovery priority, duplicates, broken overrides, dependency cycles, missing dependencies, version constraints, implicit model invocation and broken built-ins. Ground truth lives in `evals/skills/*.json`; see `skills/README.md`. |
