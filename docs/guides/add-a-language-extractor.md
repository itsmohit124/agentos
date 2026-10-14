# Add a language extractor

Parsing lives behind `LanguageExtractor` in `agentos-knowledge`. The core crates do not grow a
language-specific branch for each new language. The first registered extractor whose `matches`
returns true owns the file.

Implement the trait:

- `name` and `version` are recorded on provenance. Bump `version` when the rules change.
- `matches` looks at the path, not the contents.
- `extract` returns symbols. It does not write the database.

Register the extractor in `ExtractorRegistry::builtin`, after the existing languages. Add a small
fixture under `fixtures/` with `expected/graph.json`, and a graph case under `evals/knowledge/`.
`agentos evaluate knowledge` passes only when the scanned nodes and edges match that file exactly.

Keep command lines for tests and linters in a validation provider, not in the extractor. The
extractor describes structure. It does not run the project.
