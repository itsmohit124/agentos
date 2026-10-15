//! Cursor adapter.
//!
//! Installs `.cursor/rules/agentos.mdc` and a `beforeSubmitPrompt` hook that injects a
//! budgeted Context Pack. The rule still does not execute a workflow. Skills stay in
//! `.agents/skills/`; this adapter does not copy them into `.cursor/skills/`.
//!
//! Checked against <https://cursor.com/docs/rules> on 2026-10-14. Project rules are `.mdc`
//! files with `description`, `globs`, and `alwaysApply`. `alwaysApply: true` includes the
//! rule in every chat. Cursor also discovers `.agents/skills/`
//! (<https://cursor.com/docs/skills>). `disable-model-invocation: true` is written into a
//! user-only skill under `.agents/skills/`. That field restates AgentOS invocation. It does
//! not grant tools.

mod hooks;

use std::path::Path;

use agentos_adapter_generic::{
    AdapterCapabilities, AdapterError, AgentAdapter, InstallReport, ManagedWrite, SkillExposure,
    Support, WritePolicy, cursor_prelude, install_adapter, render_instructions,
};

/// Cursor project integration.
#[derive(Debug, Default, Clone, Copy)]
pub struct CursorAdapter;

impl CursorAdapter {
    /// Adapter id.
    pub const ID: &'static str = "cursor";
}

impl AgentAdapter for CursorAdapter {
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
        Some("cursor")
    }

    fn artifacts(&self, exposure: &SkillExposure) -> Result<Vec<ManagedWrite>, AdapterError> {
        let writes = vec![ManagedWrite {
            path: ".cursor/rules/agentos.mdc".to_owned(),
            policy: WritePolicy::Markers {
                prelude: cursor_prelude(),
                body: render_instructions(Self::ID, exposure),
            },
        }];
        Ok(writes)
    }

    fn install(&self, project: &Path) -> Result<InstallReport, AdapterError> {
        let exposure = self.expose_skills(project)?;
        let mut writes = self.artifacts(&exposure)?;
        writes.extend(hooks::artifacts(project)?);
        install_adapter(project, self.id(), &writes)
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
        assert_eq!(CRATE_NAME, "agentos-adapter-cursor");
    }

    #[test]
    fn rule_preserves_user_only_skills_and_does_not_copy_them() {
        let dir = tempfile::tempdir().unwrap();
        let skill = dir.path().join(".agents/skills/manual-only");
        fs::create_dir_all(&skill).unwrap();
        fs::write(
            skill.join("SKILL.md"),
            "---\nname: manual-only\ndescription: User only.\nversion: \"0.1.0\"\ninvocation: user\n---\n",
        )
        .unwrap();
        let adapter = CursorAdapter;
        let exposure = adapter.expose_skills(dir.path()).unwrap();
        adapter.install(dir.path()).unwrap();
        let rule = fs::read_to_string(dir.path().join(".cursor/rules/agentos.mdc")).unwrap();
        assert!(rule.starts_with("---\n"));
        assert!(rule.contains("alwaysApply: true"));
        assert!(rule.contains("<!-- agentos:begin -->"));
        assert!(invocation_violations(&rule, &exposure).is_empty());
        let model = model_section_names(&rule);
        assert!(
            !model
                .iter()
                .any(|name| name == "manual-only" || name == "grilling")
        );
        assert!(!dir.path().join(".cursor/skills").exists());
        assert!(rule.contains("does not execute a workflow"));
        let hooks = fs::read_to_string(dir.path().join(".cursor/hooks.json")).unwrap();
        assert!(hooks.contains("beforeSubmitPrompt"));
        assert!(hooks.contains("python3 .cursor/hooks/agentos-context.py"));
        assert!(
            dir.path()
                .join(".cursor/hooks/agentos-context.py")
                .is_file()
        );
        assert!(rule.contains("disable-model-invocation: true"));
        let canonical = fs::read_to_string(skill.join("SKILL.md")).unwrap();
        assert!(canonical.contains("disable-model-invocation: true"));
        assert!(canonical.contains("invocation: user"));
        let policy = fs::read_to_string(skill.join("agents/openai.yaml")).unwrap();
        assert!(policy.contains("allow_implicit_invocation: false"));
        let again = adapter.install(dir.path()).unwrap();
        assert!(
            again
                .files
                .iter()
                .all(|file| file.action == agentos_adapter_generic::FileActionKind::Unchanged)
        );
    }
}
