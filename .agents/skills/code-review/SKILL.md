---
name: code-review
description: Review a set of code changes for correctness, tests, readability and adherence to project standards, reporting findings by severity.
version: "0.1.0"
invocation: both
inputs:
  - name: changes
    description: The diff, commit range or pull request to review.
  - name: standards
    description: Project coding standards and the originating requirement.
    required: false
outputs:
  - name: review
    description: Findings with location, severity and a suggested fix.
tools: [vcs]
permissions: [fs.read, shell]
optional_recommends: [architecture-review, tdd]
triggers:
  phrases: [review the code, pull request, review my changes, review the diff, code changes]
  keywords: [review, diff, pr, bug, style]
  intents: [review]
  exclude: [architecture]
eval_cases:
  - task: Review my changes on this branch before I merge
  - task: Look over the diff for the login fix
    expect: ranked
---
# Code review

## When to use
A concrete set of changes (diff, branch, pull request) needs feedback before it is merged.

## Inputs
- The changes to review.
- Optional: the project's coding standards and the requirement the change implements.

## Procedure
1. Read the requirement, then the full diff, before commenting.
2. Check correctness: logic errors, edge cases, error handling, concurrency.
3. Check tests: changed behaviour is tested, tests can fail, no test was weakened.
4. Check standards and readability against the project's documented rules.
5. Report findings ordered by severity, each with location and a suggested fix.

## Outputs
A review: findings (blocking, should-fix, nit) with file and line, plus an overall verdict.

## Must not
- Edit the code under review.
- Report style preferences as blocking unless a documented standard requires it.
- Review architecture beyond the diff; recommend `architecture-review` instead.

## Validation
Every finding points to a specific location in the diff and states why it matters.
