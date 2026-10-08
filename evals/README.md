# Evaluations

Deterministic evaluation cases, one folder per area (see `docs/spec/EVALUATION.md`):

```text
evals/
├── knowledge/  ddd/  context/  skills/  workflow/  feedback/
├── architecture/  memory/  adapters/  end-to-end/
└── baselines/      # measured baselines; thresholds are derived from these, never invented
```

## Conventions

- One case per file, named `<topic>_<NNN>.json` (e.g. `context/payment_webhook_007.json`).
- Every case names the `fixture` it runs against and the deterministic oracle used.
- Every bug found becomes a permanent regression case in the relevant area.
- No LLM or network calls. Same input must produce the same result.
- `evals/thresholds.json` is introduced only once baselines exist (Phase 4 onward).

## Running

```bash
agentos evaluate skills      # exit 0 if every case passes, 3 if any fails
```

The harness lives in `crates/agentos-evaluation`; `cargo test --workspace` also runs the committed
suite. A case file with an unknown field, an `id` that differs from its file name, or an empty
`oracle` is rejected rather than silently skipped.

## `skills/` (Phase 1)

| `kind` | Oracle |
|---|---|
| `manifest` | one skill directory loads (or fails) with exactly the expected diagnostic codes |
| `discovery` | winning level per skill, overrides, exact diagnostic code set |
| `resolve` | exact dependency order, or the expected error code and cycle path |
| `route` | expected top skill (or `null` for none), `must_not_rank`, or a routing error |
| `permission` | `check(skill, capability)` verdict |
| `validate` | full validation outcome, diagnostic codes and (optionally) the exact skill list |
| `skill_eval` | every skill's own `eval_cases` hold (one result per eval case) |

`fixture` is a directory under `fixtures/` (with optional `project/`, `user/`, `builtin/`
subdirectories, or a bare directory of skills), or `builtin:skills` for the shipped skills.
