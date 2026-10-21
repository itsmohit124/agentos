//! Generic agent adapter and the shared contract used by Cursor, Claude Code, and Codex.
//!
//! Adapters translate AgentOS into another agent's files. Project knowledge, skills,
//! permissions, context packs, workflow plans, and feedback stay in the existing engines.

mod contract;
mod error;
mod generic;
mod instructions;
mod managed;
mod native;
mod runtime;
mod skills;
mod types;

pub use contract::AgentAdapter;
pub use error::AdapterError;
pub use generic::{GenericAdapter, claude_passthrough, cursor_prelude, safe_skill_dir};
pub use instructions::render as render_instructions;
pub use runtime::install_adapter;
pub use skills::{authorize, invocation_violations, model_section_names};
pub use types::{
    AdapterCapabilities, DoctorCheck, DoctorReport, ExposedSkill, FileAction, FileActionKind,
    InstallReport, ManagedWrite, ReportedResult, ResultRequest, SkillExposure, Support,
    UninstallReport, WorkflowInvocation, WritePolicy,
};

/// Name of this crate.
pub const CRATE_NAME: &str = env!("CARGO_PKG_NAME");

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use agentos_skills::Invoker;

    fn project() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    fn write_user_skill(root: &std::path::Path, name: &str, invocation: &str) {
        let dir = root.join(".agents/skills").join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("SKILL.md"),
            format!(
                "---\nname: {name}\ndescription: Adapter test skill.\nversion: \"0.1.0\"\ninvocation: {invocation}\n---\n"
            ),
        )
        .unwrap();
    }

    #[test]
    fn crate_name_is_stable() {
        assert_eq!(CRATE_NAME, "agentos-adapter-generic");
    }

    #[test]
    fn user_only_skill_stays_user_only_and_model_is_denied() {
        let dir = project();
        write_user_skill(dir.path(), "manual-only", "user");
        write_user_skill(dir.path(), "either", "both");
        let adapter = GenericAdapter;
        let exposure = adapter.expose_skills(dir.path()).unwrap();
        let manual = exposure
            .skills
            .iter()
            .find(|skill| skill.name == "manual-only")
            .unwrap();
        assert!(manual.user_invocable);
        assert!(!manual.model_invocable);
        let either = exposure
            .skills
            .iter()
            .find(|skill| skill.name == "either")
            .unwrap();
        assert!(either.user_invocable && either.model_invocable);
        let grilling = exposure
            .skills
            .iter()
            .find(|skill| skill.name == "grilling")
            .unwrap();
        assert!(grilling.user_invocable && !grilling.model_invocable);

        adapter.install(dir.path()).unwrap();
        let text = fs::read_to_string(dir.path().join(".agentos/adapters/generic/INSTRUCTIONS.md"))
            .unwrap();
        assert!(invocation_violations(&text, &exposure).is_empty());
        let model = model_section_names(&text);
        assert!(model.contains(&"either".to_owned()));
        assert!(!model.contains(&"manual-only".to_owned()));
        assert!(!model.contains(&"grilling".to_owned()));
        assert!(authorize(dir.path(), "manual-only", Invoker::User).is_ok());
        assert!(authorize(dir.path(), "manual-only", Invoker::Model).is_err());

        let index =
            fs::read_to_string(dir.path().join(".agentos/adapters/generic/skills.json")).unwrap();
        let value: serde_json::Value = serde_json::from_str(&index).unwrap();
        let row = value["skills"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["name"] == "manual-only")
            .unwrap();
        assert_eq!(row["model_invocable"], false);
        assert_eq!(row["user_invocable"], true);
    }

    #[test]
    fn claimed_pass_is_rejected_and_missing_shell_is_refused() {
        let dir = project();
        fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"probe\"\nversion = \"0.0.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        let adapter = GenericAdapter;
        let claim = adapter.report_result(
            dir.path(),
            &ResultRequest {
                kind: "unit_test".to_owned(),
                task_id: None,
                context_pack_id: None,
                claimed_outcome: Some("passed".to_owned()),
                permissions: vec!["shell".to_owned()],
            },
        );
        assert!(matches!(claim, Err(AdapterError::UnverifiedClaim(_))));
        let refused = adapter
            .report_result(
                dir.path(),
                &ResultRequest {
                    kind: "unit_test".to_owned(),
                    task_id: Some("task".to_owned()),
                    context_pack_id: None,
                    claimed_outcome: None,
                    permissions: Vec::new(),
                },
            )
            .unwrap();
        assert!(!refused.passed);
        assert_eq!(refused.outcome, "refused");
        assert!(refused.executed);
    }

    #[test]
    fn context_workflow_and_architecture_use_the_engines() {
        let dir = project();
        fs::write(dir.path().join("README.md"), "# sample\n").unwrap();
        let adapter = GenericAdapter;
        let task = "Add retry support to payment webhook processing";
        let pack = adapter.provide_context(dir.path(), task).unwrap();
        assert_eq!(pack.task, task);
        let planned = adapter
            .invoke_workflow(dir.path(), task, Some("feature"))
            .unwrap();
        assert_eq!(planned.workflow, "feature");
        assert!(!planned.plan_id.is_empty());
        assert!(!planned.steps.is_empty());
        let feedback = adapter
            .report_result(
                dir.path(),
                &ResultRequest {
                    kind: "architecture".to_owned(),
                    task_id: None,
                    context_pack_id: None,
                    claimed_outcome: None,
                    permissions: Vec::new(),
                },
            )
            .unwrap();
        assert!(feedback.executed);
        assert_eq!(feedback.source, "architecture");
        assert!(feedback.detail.starts_with("findings="));
    }

    #[test]
    fn capabilities_do_not_advertise_an_external_agent() {
        let caps = GenericAdapter.capabilities();
        assert_eq!(caps.external_agent, Support::Unsupported);
        assert_eq!(caps.context, Support::Supported);
        assert!(GenericAdapter.external_command().is_none());
    }

    #[test]
    fn install_is_idempotent() {
        let dir = project();
        let adapter = GenericAdapter;
        adapter.install(dir.path()).unwrap();
        let again = adapter.install(dir.path()).unwrap();
        assert!(
            again
                .files
                .iter()
                .all(|file| file.action == FileActionKind::Unchanged)
        );
    }
}
