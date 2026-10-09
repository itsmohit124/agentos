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
agentos evaluate knowledge
agentos evaluate ddd
agentos evaluate context
agentos benchmark baseline fixtures/python-fastapi
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

## `knowledge/` (Phase 2 and 2b)

Phase 2: four profile cases and the `python-fastapi` graph. Phase 2b adds graph cases
`node-express`, `typescript-react`, `go`, `java`, and `ruby-rails`. A case `id` is the file stem.

| `kind` | Oracle |
|---|---|
| `profile` | `fixtures/<fixture>/expected/profile.json` (classification, maturity, confidence, every signal) |
| `graph` | `fixtures/<fixture>/expected/graph.json` (node id, type, provenance kind; edge endpoints, relation, provenance kind) |

A graph case passes only when both sets match (recall and precision are `n/n`), every node and edge
has complete provenance, a second scan writes zero rows, and deleting `agentos.db` then scanning
reproduces the same node fingerprints. The case copies the fixture before scanning it.

## `ddd/` (Phase 3)

| `kind` | Oracle |
|---|---|
| `domain` | `fixtures/<fixture>/expected/domain.json` (node id, type, lifecycle, provenance; edge endpoints, relation, provenance; finding id and kind) |
| `markdown` | `fixtures/<fixture>/expected/domain-markdown/*.md` after `domain scan`, `domain propose`, and `domain confirm` of the case's `confirm` id |

A domain case passes only when all three sets match. The detail line reports node, edge, and finding
precision and recall, plus confirmed and canonical counts (both zero on these cases) and whether
provenance is complete. A markdown case passes when the five generated files match the golden bytes.
No timestamp is part of either oracle.

## `context/` (Phase 4)

| `kind` | Oracle |
|---|---|
| `context` | `must_include`, `must_exclude`, `required_fragments` on a Context Pack built from `fixtures/<fixture>` after scan and domain scan |

`relevant_files` is the manual file list for `agentos benchmark baseline`. The context engine does
not read it. `max_tokens` and `max_irrelevant_ratio` are the measured pack size and irrelevant-token
ratio. A bounded-context id in `must_exclude` also rejects every selected node in that package.
Recall of `must_include` and `required_fragments` must be complete. The detail line reports recall,
precision, tokens, irrelevant tokens, latency, and reproducibility.
