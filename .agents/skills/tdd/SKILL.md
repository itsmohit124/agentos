---
name: tdd
description: Test-driven development - write a failing test first, make it pass with the smallest change, then refactor, one behaviour at a time.
version: "0.1.0"
invocation: both
requires:
  - name: interface-design
    version: "^0.1"
inputs:
  - name: behaviour
    description: The behaviour to add or the bug to fix, stated as observable outcomes.
  - name: interface
    description: The interface under test (from interface-design).
outputs:
  - name: tests
    description: New tests that failed before the change and pass after it.
  - name: change
    description: The minimal code change that makes them pass.
tools: [test-runner]
permissions: [fs.read, fs.write, shell]
references: [references/test-list.md]
optional_recommends: [code-review]
triggers:
  phrases: [write tests first, test first, test driven, red green refactor, failing test, regression test]
  keywords: [tdd, test, failing, regression]
  intents: [testing]
eval_cases:
  - task: Start with a failing test for the discount calculation
  - task: Follow red green refactor for the token parser
  - task: Grill me about the rollout
    expect: not_ranked
---
# Test-driven development

## When to use
Adding behaviour or fixing a bug where the expected outcome can be stated as a test.

## Inputs
- The behaviour or bug, stated as observable outcomes.
- The interface under test (produced by `interface-design`, which runs first).

## Procedure
1. Write a test list for the behaviour using `references/test-list.md`.
2. Pick the simplest untested item and write one failing test for it.
3. Run the tests and confirm the new test fails for the expected reason.
4. Make the smallest change that makes all tests pass.
5. Refactor while keeping all tests green.
6. Repeat from step 2 until the test list is empty.

## Outputs
Tests that failed before and pass after, plus the minimal change. For bugs, a permanent
regression test.

## Must not
- Write production code without a failing test that demands it.
- Weaken or delete a test to make it pass.
- Change several behaviours in one cycle.

## Validation
The full test suite passes, and each new test was observed failing before its change.
