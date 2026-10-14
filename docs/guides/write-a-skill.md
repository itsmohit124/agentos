# Write a skill

A skill is a directory whose `SKILL.md` has YAML front-matter and a markdown body. The schema is
`schemas/skill-manifest.schema.json`.

```markdown
---
name: retry-webhook
description: Add retry around an existing payment webhook.
version: "0.1.0"
permissions: [fs.read]
---

# Retry webhook

Use the existing Payment term. Do not edit unrelated contexts.
```

`version` is a semantic version. A dependency may constrain it, for example `version: "^0.1"`.
A manifest written before `disable-model-invocation` existed still loads. That field is optional
and cannot make a user-only skill model-invocable.

`user_invocable` defaults to true. `model_invocable` defaults to false. A model-invocable skill
must not require a user-only skill. Permissions that are not listed are denied.

Install a local directory into the project:

```bash
agentos skills install ./retry-webhook
agentos skills validate
```

Project skills live in `.agents/skills/<name>/`. The directory name must equal `name`. A higher
discovery level overrides a lower one. The body is not read until something inspects or validates
the skill.

Check the skill with `agentos evaluate skills` after adding a case under `evals/skills/`.
