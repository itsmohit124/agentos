//! Codex adapter.
//!
//! Manages an AgentOS section in `AGENTS.md`. Codex already discovers `.agents/skills/`,
//! so this adapter does not copy skills into a second directory.
//!
//! Checked against <https://developers.openai.com/codex/guides/agents-md> and
//! <https://developers.openai.com/codex/skills> on 2026-10-14. Codex reads `AGENTS.md`
//! from the project root and scans `.agents/skills` for `SKILL.md`. Skill discovery uses
//! `name` and `description`. User-only skills also get `agents/openai.yaml` with
//! `allow_implicit_invocation: false`, which is Codex's documented implicit-invocation
//! control. The managed section does not execute a command.

use agentos_adapter_generic::{
    AdapterCapabilities, AdapterError, AgentAdapter, ManagedWrite, SkillExposure, Support,
    WritePolicy, render_instructions,
};

/// Codex project integration.
#[derive(Debug, Default, Clone, Copy)]
pub struct CodexAdapter;

impl CodexAdapter {
    /// Adapter id.
    pub const ID: &'static str = "codex";
}

impl AgentAdapter for CodexAdapter {
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
        Some("codex")
    }

    fn artifacts(&self, exposure: &SkillExposure) -> Result<Vec<ManagedWrite>, AdapterError> {
        Ok(vec![ManagedWrite {
            path: "AGENTS.md".to_owned(),
            policy: WritePolicy::Markers {
                prelude: String::new(),
                body: render_instructions(Self::ID, exposure),
            },
        }])
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
        assert_eq!(CRATE_NAME, "agentos-adapter-codex");
    }

    #[test]
    fn agents_section_preserves_user_text_and_user_only_skills() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("AGENTS.md"),
            "# Team rules\n\nLeave this.\n",
        )
        .unwrap();
        let original = fs::read(dir.path().join("AGENTS.md")).unwrap();
        let adapter = CodexAdapter;
        let exposure = adapter.expose_skills(dir.path()).unwrap();
        adapter.install(dir.path()).unwrap();
        let text = fs::read_to_string(dir.path().join("AGENTS.md")).unwrap();
        assert!(text.starts_with("# Team rules\n\nLeave this.\n"));
        assert!(text.contains("<!-- agentos:begin -->"));
        assert!(invocation_violations(&text, &exposure).is_empty());
        assert!(text.contains("allow_implicit_invocation: false"));
        assert!(
            !model_section_names(&text)
                .iter()
                .any(|name| name == "grilling")
        );
        assert!(!dir.path().join(".codex").exists());
        adapter.uninstall(dir.path()).unwrap();
        assert_eq!(fs::read(dir.path().join("AGENTS.md")).unwrap(), original);
    }
}
