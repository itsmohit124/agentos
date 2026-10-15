---
name: improve-codebase-architecture
description: Improve the structure of a codebase in small, behaviour-preserving steps - deepen shallow modules, fix dependency direction, reduce coupling.
version: "0.1.0"
invocation: user
disable-model-invocation: true
requires:
  - name: architecture-review
    version: "^0.1"
inputs:
  - name: findings
    description: The architecture review findings to address (from architecture-review).
  - name: goal
    description: The structural outcome the human wants.
outputs:
  - name: changes
    description: A sequence of small refactorings, each with passing checks.
  - name: summary
    description: Before/after structure and remaining findings.
tools: [test-runner, vcs]
permissions: [fs.read, fs.write, shell]
optional_recommends: [interface-design]
triggers:
  phrases: [improve the architecture, improve the codebase, codebase architecture, shallow module, reduce coupling]
  keywords: [improve, refactor, deepen, untangle, restructure, shallow]
  intents: [restructure]
eval_cases:
  - task: Refactor to untangle the billing and shipping modules
  - task: Improve the codebase by deepening shallow modules
---
# Improve codebase architecture

## When to use
The human has decided to restructure code, usually after an `architecture-review` (which runs
first). This skill is user-invoked only because it makes broad changes.

## Inputs
- Findings from `architecture-review`.
- The structural goal.

## Procedure
1. Pick the highest-impact finding and state the target structure for it.
2. Make sure tests cover the behaviour of the code that will move; add tests first if not.
3. Apply one behaviour-preserving refactoring step.
4. Run the full checks; revert the step if anything fails.
5. Repeat until the finding is resolved, then re-run `architecture-review` on the scope.

## Outputs
Small, separately verified refactoring steps and a before/after summary.

## Must not
- Change behaviour while restructuring.
- Combine refactoring with feature work.
- Start without an explicit request from the human.

## Validation
All checks pass after every step, and the follow-up architecture review no longer reports the
addressed finding.
