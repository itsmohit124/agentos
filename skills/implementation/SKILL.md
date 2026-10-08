---
name: implementation
description: Implement a scoped change end to end - plan the steps, make small verified edits, and leave the build and tests green.
version: "0.1.0"
invocation: both
inputs:
  - name: task
    description: The change to make and its acceptance criteria.
  - name: context
    description: Relevant code, interfaces and constraints.
    required: false
outputs:
  - name: change
    description: The code change, with tests, passing the project's checks.
  - name: summary
    description: What changed, why, and how it was verified.
tools: [test-runner, build]
permissions: [fs.read, fs.write, shell]
optional_recommends: [tdd, code-review]
triggers:
  phrases: [implement, add support, add a feature, build the feature, wire up, make it work]
  keywords: [implement, add, build, feature, support]
  intents: [implement]
eval_cases:
  - task: Implement CSV export for the reports page
  - task: Wire up the new settings screen to the API client
    expect: ranked
---
# Implementation

## When to use
A change is scoped and understood well enough to build: acceptance criteria exist and no open
design question blocks it.

## Inputs
- The task and its acceptance criteria.
- Optional context: relevant code, interfaces, constraints.

## Procedure
1. Restate the acceptance criteria and list the files likely to change.
2. Break the work into small steps that each leave the build green.
3. For each step, make the edit, then run the relevant tests and checks.
4. Add or update tests for every behaviour changed.
5. Run the full check suite and summarise what changed and how it was verified.

## Outputs
A change that meets the acceptance criteria with passing checks, and a short summary.

## Must not
- Expand scope beyond the task or refactor unrelated code.
- Leave failing or skipped tests behind.
- Claim success without running the checks.

## Validation
The project's build, lint and test commands pass, and each acceptance criterion maps to a test or
an observed result.
