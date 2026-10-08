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

Phase 0 only establishes this layout; no evaluation cases exist yet.
