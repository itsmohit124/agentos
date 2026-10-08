---
name: interface-design
description: Design a small, deep public interface (API, trait, module boundary) and its contract tests before any implementation exists.
version: "0.1.0"
invocation: both
inputs:
  - name: responsibility
    description: What the module must do for its callers, and who the callers are.
  - name: constraints
    description: Performance, compatibility or dependency constraints.
    required: false
outputs:
  - name: interface
    description: The proposed public interface with documented behaviour and error cases.
  - name: contract_tests
    description: Test cases every implementation of the interface must pass.
permissions: [fs.read, fs.write]
optional_recommends: [domain-modeling]
triggers:
  phrases: [public api, design the interface, design an interface, deep module, function signature]
  keywords: [interface, api, signature, contract, seam, trait]
  intents: [design]
eval_cases:
  - task: Design the interface for the storage adapter
  - task: Propose a trait with a small surface for the event bus
    expect: ranked
---
# Interface design

## When to use
Before implementing a new module, or when an existing module's interface leaks its internals or
forces callers to know too much.

## Inputs
- The responsibility of the module and its callers.
- Optional constraints (performance, compatibility, allowed dependencies).

## Procedure
1. Write down what callers need, in their words, without naming implementation details.
2. Propose the smallest interface that serves those needs; hide everything else.
3. Document each operation: inputs, outputs, errors, and invariants.
4. Write contract tests that any implementation must pass, including error cases.
5. Compare with one alternative design and record why this one was chosen.

## Outputs
The interface (signatures plus documentation) and its contract tests, ready for `tdd`.

## Must not
- Implement the module.
- Expose internal types or configuration callers do not need.
- Skip the error cases in the contract tests.

## Validation
The contract tests compile against the interface and fail only because no implementation exists.
