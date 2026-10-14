# Add an adapter

An adapter installs instructions for one coding agent and calls the existing AgentOS commands. It
plans a workflow. It does not run one, and it does not grant a capability the skill did not declare.

The contract is `AgentAdapter` in `agentos-adapter-generic`:

- `install` and `uninstall` change only managed files
- `expose_skills` lists the registry without loading bodies
- `provide_context` returns a Context Pack
- `invoke_workflow` returns a plan and does not create `.agentos/runs`
- `report_result` rejects a claimed pass
- `doctor` checks the install without starting the external agent

Markdown instructions use `<!-- agentos:begin -->` and `<!-- agentos:end -->`. The manifest at
`.agentos/adapters/manifest.json` records the path. Two adapters do not share a path. Uninstall
removes the managed block and leaves text outside it in place.

A user-only skill stays user-only. Do not copy it into a second skill tree, do not set
`allowed-tools`, and do not mark it model-invocable. Host controls such as
`disable-model-invocation: true` restate the AgentOS flag. `agentos cleanup` does not remove them.

Add cases under `evals/adapters/` and run `agentos evaluate adapters`. A missing external binary
is `SKIP`. That skip is not a pass. Do not start the agent process from the adapter.
