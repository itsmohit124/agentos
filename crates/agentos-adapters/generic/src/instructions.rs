//! Instruction text shared by every adapter.
//!
//! The text tells an agent when to call `agentos`. It does not execute a command, and it does
//! not grant a permission.

use crate::types::SkillExposure;

/// Markdown body placed inside an AgentOS-managed block.
pub fn render(adapter: &str, exposure: &SkillExposure) -> String {
    let mut out = String::new();
    out.push_str(&format!("# AgentOS ({adapter})\n\n"));
    out.push_str(
        "AgentOS is the source of truth for skills, permissions, context, workflow, and validation.\n",
    );
    out.push_str(
        "This file tells the agent when to call the `agentos` CLI. It does not run those commands.\n",
    );
    out.push_str(
        "A normal chat is the same as the CLI: do not skip AgentOS because the human typed in the agent instead of the terminal.\n\n",
    );
    out.push_str("## Every task\n\n");
    out.push_str("Before editing **or** walking the tree, for the user's current task (including `/improve-codebase-architecture` and `/architecture-review`):\n");
    out.push_str("1. Run `agentos scan` if the project has not been scanned or files changed.\n");
    out.push_str("2. Run `agentos context build \"<task>\"` with the user's request as `<task>`.\n");
    out.push_str("3. For architecture work, also run `agentos architecture analyze` and `agentos skills inspect <skill>`.\n");
    out.push_str("4. Follow that Context Pack. Do not start from an unguided repo search when `agentos` is installed.\n");
    out.push_str("5. `--root` is the application Git root. Do not scan a nested AgentOS source tree unless that is the project.\n");
    out.push_str("6. User-only skills stay user-invoked (`/grilling`, `/setup-agentos`, `/improve-codebase-architecture`, and the rest in that list).\n\n");
    out.push_str("## Commands\n\n");
    out.push_str("- Build a Context Pack: `agentos context build \"<task>\"`\n");
    out.push_str("- Architecture: `agentos architecture analyze`\n");
    out.push_str("- Validate the project: `agentos validate --kind unit_test`\n");
    out.push_str("- Plan a workflow: `agentos plan \"<task>\"`\n");
    out.push_str("- Execute a saved plan: `agentos run <plan-id> --runner manual`\n\n");
    out.push_str("Pass `--root <project>` when the working directory is not the project root.\n\n");
    out.push_str("## Invocation policy\n\n");
    out.push_str(
        "`user_invocable` and `model_invocable` stay independent. A user-invoked-only skill must not be selected by the model.\n",
    );
    out.push_str(
        "AgentOS denies that call. Permissions are default-deny and are checked by AgentOS.\n",
    );
    out.push_str(
        "Nothing in this file grants `fs.read`, `fs.write`, `shell`, `network`, or `browser`.\n\n",
    );
    out.push_str("## Model-invocable skills\n\n");
    push_skills(
        &mut out,
        exposure.skills.iter().filter(|skill| skill.model_invocable),
    );
    out.push_str("\n## User-invoked only\n\n");
    push_skills(
        &mut out,
        exposure
            .skills
            .iter()
            .filter(|skill| skill.user_invocable && !skill.model_invocable),
    );
    out.push_str("\n## Skills location\n\n");
    out.push_str(
        "Canonical skills are portable `SKILL.md` packages under `.agents/skills/` and `.agentos/skills/`.\n",
    );
    out.push_str(
        "Built-in skills are compiled into `agentos`. Do not copy a skill to change who may invoke it.\n\n",
    );
    match adapter {
        "cursor" => out.push_str(
            "Cursor discovers `.agents/skills/` and does not read the AgentOS `invocation` field.\n\
             Install writes `disable-model-invocation: true` into a user-only skill's front matter. That is Cursor's documented control.\n\
             It does not grant tools and it does not make a user-only skill model-invocable.\n\
             This adapter does not copy skills into `.cursor/skills/`.\n\
             The rule file does not execute `agentos`; the agent must run the command.\n",
        ),
        "claude" => out.push_str(
            "Passthrough files under `.claude/skills/<name>/SKILL.md` set `disable-model-invocation` from the AgentOS flag.\n\
             They do not set `allowed-tools`, because that field would grant tools for a turn.\n\
             That field is the documented Claude Code control for those passthrough files. It was not executed against the Claude binary.\n\
             `CLAUDE.md` does not execute `agentos`; the agent must run the command.\n",
        ),
        "codex" => out.push_str(
            "Codex reads this `AGENTS.md` section and discovers `.agents/skills/`.\n\
             Install writes `agents/openai.yaml` with `allow_implicit_invocation: false` for a user-only skill.\n\
             That is Codex's documented control. It does not grant tools and it does not change who may invoke the skill in AgentOS.\n\
             This adapter does not duplicate the skill body.\n\
             This section does not execute `agentos`.\n",
        ),
        _ => out.push_str(
            "Any agent that can read files and run commands can use this project.\n\
             `.agentos/adapters/generic/skills.json` is a generated index, not a second registry.\n",
        ),
    }
    out
}

fn push_skills<'a>(out: &mut String, skills: impl Iterator<Item = &'a crate::types::ExposedSkill>) {
    let mut any = false;
    for skill in skills {
        any = true;
        let permissions = if skill.permissions.is_empty() {
            "(none)".to_owned()
        } else {
            skill.permissions.join(",")
        };
        out.push_str(&format!(
            "- `{}` user_invocable={} model_invocable={} permissions={permissions}\n",
            skill.name, skill.user_invocable, skill.model_invocable
        ));
    }
    if !any {
        out.push_str("(none)\n");
    }
}

/// Claude Code discovery file. It points at the canonical skill and does not grant tools.
pub fn claude_passthrough(skill: &crate::types::ExposedSkill) -> String {
    let description = yaml_quote(&skill.description);
    format!(
        "---\n\
         name: {name}\n\
         description: {description}\n\
         disable-model-invocation: {disable}\n\
         user-invocable: {user}\n\
         ---\n\
         \n\
         # {name}\n\
         \n\
         Canonical AgentOS skill: `{path}`.\n\
         \n\
         This file is a discovery passthrough. It does not grant tools and it does not change the skill.\n\
         Invocation: user_invocable={user} model_invocable={model}.\n\
         Permissions recorded by AgentOS: {permissions}.\n\
         Those names are not a grant. AgentOS checks them when a command runs.\n\
         Read the canonical `SKILL.md` for the procedure.\n",
        name = skill.name,
        path = skill.path,
        disable = !skill.model_invocable,
        user = skill.user_invocable,
        model = skill.model_invocable,
        permissions = if skill.permissions.is_empty() {
            "(none)".to_owned()
        } else {
            skill.permissions.join(", ")
        },
    )
}

fn yaml_quote(value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

/// Cursor rule frontmatter. It stays outside the managed block so the file starts with `---`.
pub fn cursor_prelude() -> String {
    "---\n\
     description: Use AgentOS for project context, validation, and skill invocation policy. This rule does not execute commands.\n\
     alwaysApply: true\n\
     ---\n"
        .to_owned()
}

/// A skill directory name that is safe to place under `.claude/skills/`.
pub fn safe_skill_dir(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
}
