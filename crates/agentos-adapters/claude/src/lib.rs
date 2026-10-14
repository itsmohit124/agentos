//! Claude Code adapter.
//!
//! Manages an AgentOS section in `CLAUDE.md` and writes a discovery passthrough at
//! `.claude/skills/<name>/SKILL.md`. The passthrough sets `disable-model-invocation` and
//! `user-invocable` from the AgentOS manifest. It does not set `allowed-tools`.
//!
//! Checked against <https://code.claude.com/docs/en/skills> and
//! <https://code.claude.com/docs/en/claude-md> on 2026-10-14. Project instructions live in
//! `CLAUDE.md`. Skills live in `.claude/skills/<name>/SKILL.md`. No hooks are installed:
//! a hook would execute a command, and these files only describe when to call `agentos`.

use agentos_adapter_generic::{
    AdapterCapabilities, AdapterError, AgentAdapter, ManagedWrite, SkillExposure, Support,
    WritePolicy, claude_passthrough, render_instructions, safe_skill_dir,
};

/// Claude Code project integration.
#[derive(Debug, Default, Clone, Copy)]
pub struct ClaudeAdapter;

impl ClaudeAdapter {
    /// Adapter id.
    pub const ID: &'static str = "claude";
}

impl AgentAdapter for ClaudeAdapter {
    fn id(&self) -> &'static str {
        Self::ID
    }

    fn capabilities(&self) -> AdapterCapabilities {
        AdapterCapabilities {
            id: Self::ID.to_owned(),
            install: Support::Supported,
            skills: Support::Supported,
            context: Support::Supported,
            workflow: Support::Supported,
            feedback: Support::Supported,
            external_agent: Support::Optional,
        }
    }

    fn external_command(&self) -> Option<&'static str> {
        Some("claude")
    }

    fn artifacts(&self, exposure: &SkillExposure) -> Result<Vec<ManagedWrite>, AdapterError> {
        let mut files = vec![ManagedWrite {
            path: "CLAUDE.md".to_owned(),
            policy: WritePolicy::Markers {
                prelude: String::new(),
                body: render_instructions(Self::ID, exposure),
            },
        }];
        for skill in &exposure.skills {
            if !safe_skill_dir(&skill.name) {
                return Err(AdapterError::Skills(format!(
                    "skill name {:?} cannot be a Claude skill directory",
                    skill.name
                )));
            }
            files.push(ManagedWrite {
                path: format!(".claude/skills/{}/SKILL.md", skill.name),
                policy: WritePolicy::Whole {
                    contents: claude_passthrough(skill),
                },
            });
        }
        Ok(files)
    }
}

/// Name of this crate.
pub const CRATE_NAME: &str = env!("CARGO_PKG_NAME");

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use agentos_adapter_generic::{invocation_violations, model_section_names};

    #[test]
    fn crate_name_is_stable() {
        assert_eq!(CRATE_NAME, "agentos-adapter-claude");
    }

    #[test]
    fn passthrough_keeps_user_only_skills_user_only() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("CLAUDE.md"), "user notes\n").unwrap();
        let adapter = ClaudeAdapter;
        let exposure = adapter.expose_skills(dir.path()).unwrap();
        adapter.install(dir.path()).unwrap();
        let claude_md = fs::read_to_string(dir.path().join("CLAUDE.md")).unwrap();
        assert!(claude_md.starts_with("user notes\n"));
        assert!(claude_md.contains("<!-- agentos:begin -->"));
        assert!(invocation_violations(&claude_md, &exposure).is_empty());
        assert!(
            !model_section_names(&claude_md)
                .iter()
                .any(|name| name == "grilling")
        );
        let grilling =
            fs::read_to_string(dir.path().join(".claude/skills/grilling/SKILL.md")).unwrap();
        assert!(grilling.contains("disable-model-invocation: true"));
        assert!(grilling.contains("user-invocable: true"));
        assert!(!grilling.contains("allowed-tools"));
        let tdd = fs::read_to_string(dir.path().join(".claude/skills/tdd/SKILL.md")).unwrap();
        assert!(tdd.contains("disable-model-invocation: false"));
        assert!(!dir.path().join(".claude/settings.json").exists());
        let again = adapter.install(dir.path()).unwrap();
        assert!(
            again
                .files
                .iter()
                .all(|file| file.action == agentos_adapter_generic::FileActionKind::Unchanged)
        );
    }
}
