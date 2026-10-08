# Fixture: skills

Skill trees with known ground truth for the skill-system evaluations (`evals/skills/`). The ground
truth for every scenario lives in the evaluation case that names it (`expect_*` fields), so this
fixture has no `expected/` directory.

Each scenario directory is laid out like a real setup:

```text
<scenario>/
├── project/.agents/skills/<skill>/SKILL.md    # level: project
├── project/.agentos/skills/<skill>/SKILL.md   # level: agentos
├── user/<skill>/SKILL.md                      # level: user
└── builtin/<skill>/SKILL.md                   # level: builtin (stricter rules)
```

`manifests/<case>/` directories contain a single skill each and exercise manifest parsing.

| Scenario | What it contains |
|---|---|
| `manifests/*` | one valid or deliberately invalid manifest per case |
| `override` | the same names at several levels (priority and overrides) |
| `duplicate` | two project skills claiming the same name |
| `broken-override` | an invalid project skill hiding a valid built-in |
| `cycle` | `a` requires `b`, `b` requires `a` |
| `missing-dep` | `a` requires a skill that does not exist |
| `version-mismatch` | constraints that do and do not match `base 0.1.0` |
| `implicit-model` | a model-invocable skill requiring a user-invoked-only skill |
| `broken-builtin` | built-ins violating the quality rule and missing eval cases |
