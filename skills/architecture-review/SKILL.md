---
name: architecture-review
description: Assess a codebase's structure - module boundaries, dependency direction, layering and coupling - and report violations with evidence.
version: "0.1.0"
invocation: both
inputs:
  - name: scope
    description: The modules or directories to assess.
  - name: rules
    description: Intended architecture rules (layers, allowed dependencies).
    required: false
outputs:
  - name: report
    description: Violations and risks with evidence (file, import, call), ranked by impact.
permissions: [fs.read]
optional_recommends: [domain-modeling]
triggers:
  phrases: [review the architecture, dependency direction, layering violation, module boundaries, coupling]
  keywords: [architecture, layering, coupling, cohesion, dependency]
  intents: [architecture]
  exclude: [improve, refactor, deepen]
eval_cases:
  - task: Check the module boundaries and coupling of the billing service
  - task: Improve the architecture of the payments package
    expect: not_ranked
---
# Architecture review

## When to use
You need an evidence-based picture of how the code is structured: before a large change, after
growth, or when boundaries seem to be eroding. This skill only assesses; it does not change code.

## Inputs
- The scope to assess.
- Optional intended rules (layers, allowed dependency directions).

## Procedure
0. If this project has `.agentos/` or `agentos` on PATH, do **not** start by walking the
   tree. `--root` is this application, not a vendored AgentOS checkout. Run:
   `agentos scan` (if the graph is missing or stale), then
   `agentos context build "<the user's task>"`, then
   `agentos architecture analyze`.
   Use that output as the module map and as evidence. Only open files the CLI cited.
1. If AgentOS is not installed, map the modules in scope and the dependencies between them.
2. Compare the dependency directions with the intended rules (or infer and state them).
3. Identify cycles, layering violations, high coupling and shallow modules.
4. For each finding, record concrete evidence (file, import or call site), preferring
   AgentOS node/edge ids when a scan exists.
5. Rank findings by impact and suggest which to address first.

## Outputs
A report of violations and risks with evidence, ranked by impact.

## Must not
- Modify code; use `improve-codebase-architecture` for changes.
- Report a violation without evidence.
- Assume a framework or language convention without checking the code.
- Walk the whole tree first when AgentOS is installed in this project.

## Validation
Every finding cites evidence a reviewer can open, and the dependency map covers the whole scope.
