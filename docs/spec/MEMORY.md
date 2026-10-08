# Memory Contract

Memory is persistent project knowledge and uses the same node/edge/context infrastructure as the Knowledge Engine. It is never a separate opaque retrieval path.

## Memory types

- episodic: what happened during a task/run
- semantic: stable facts learned about the project
- procedural: how the project/team performs work
- decision: explicit engineering/domain decisions
- lesson: reusable knowledge derived from failures or reviews

## Provenance

Every memory record must identify its origin, such as task/run id, source artifact, outcome, and timestamp where appropriate.

## Trust

New memories are proposed unless explicitly confirmed. Memory must not silently become project truth.

## Staleness

Memory can be linked to source/node fingerprints. If those fingerprints change, mark the memory stale and down-rank it rather than deleting it automatically.

## Privacy

V1 memory is local-only. Secret-like content must be redacted before persistence. Cross-project memory is out of scope for V1.
