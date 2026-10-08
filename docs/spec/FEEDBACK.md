# Feedback Engine Contract

The Feedback Engine turns objective engineering signals into structured feedback that can be consumed by workflows and agents.

## Provider model

Providers implement capabilities such as:
- test execution
- static type checking
- linting
- build/compile
- browser validation
- architecture checks

The core asks for capabilities; technology adapters supply the project-specific implementation.

## Feedback result

Every validation result should record:
- provider kind
- target
- command/capability used
- exit status
- pass/fail
- structured diagnostics when available
- relevant artifacts
- provenance

## Repair loop

A workflow may route failed validation back to implementation with a bounded retry count. Tests must not be weakened merely to make a workflow pass.

## Architecture feedback

Architecture validation should detect evidence-based violations such as boundary leakage, unwanted dependency direction, circular dependencies, oversized public surfaces, and domain/infrastructure coupling.
