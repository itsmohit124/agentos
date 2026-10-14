//! The adapter contract. Agent-specific crates supply files; these methods call AgentOS.

use std::path::Path;

use crate::error::AdapterError;
use crate::runtime;
use crate::skills;
use crate::types::{
    AdapterCapabilities, DoctorReport, InstallReport, ManagedWrite, ReportedResult, ResultRequest,
    SkillExposure, UninstallReport, WorkflowInvocation,
};

/// Integration surface for one coding agent.
///
/// `install`, `expose_skills`, `provide_context`, `report_result`, and `capabilities` are the
/// contract from the adapter spec. `uninstall`, `doctor`, and `invoke_workflow` are the extra
/// operations the CLI and the compatibility chain require. They use the same engines.
pub trait AgentAdapter: Send + Sync {
    /// Stable id: `generic`, `cursor`, `claude`, or `codex`.
    fn id(&self) -> &'static str;

    /// Static capabilities. This does not probe the network or an external agent.
    fn capabilities(&self) -> AdapterCapabilities;

    /// External program name, when this adapter has an optional one.
    fn external_command(&self) -> Option<&'static str>;

    /// Files this adapter manages for the current skill exposure.
    fn artifacts(&self, exposure: &SkillExposure) -> Result<Vec<ManagedWrite>, AdapterError>;

    /// Installs or updates managed files. Repeated calls are idempotent.
    fn install(&self, project: &Path) -> Result<InstallReport, AdapterError> {
        let exposure = self.expose_skills(project)?;
        let artifacts = self.artifacts(&exposure)?;
        runtime::install_adapter(project, self.id(), &artifacts)
    }

    /// Removes managed content and leaves user content in place.
    fn uninstall(&self, project: &Path) -> Result<UninstallReport, AdapterError> {
        runtime::uninstall_adapter(project, self.id())
    }

    /// Projects the skill registry. Skill bodies are not copied.
    fn expose_skills(&self, project: &Path) -> Result<SkillExposure, AdapterError> {
        skills::expose(project)
    }

    /// Context Pack for `task`, from the context engine.
    fn provide_context(
        &self,
        project: &Path,
        task: &str,
    ) -> Result<agentos_context::ContextPack, AdapterError> {
        runtime::provide_context(project, task)
    }

    /// Forwards an outcome to the feedback engine.
    fn report_result(
        &self,
        project: &Path,
        request: &ResultRequest,
    ) -> Result<ReportedResult, AdapterError> {
        runtime::report_result(project, request)
    }

    /// Plans `task` with the workflow engine. Does not start a run.
    fn invoke_workflow(
        &self,
        project: &Path,
        task: &str,
        workflow: Option<&str>,
    ) -> Result<WorkflowInvocation, AdapterError> {
        runtime::invoke_workflow(project, task, workflow)
    }

    /// Checks the installation without modifying the project.
    fn doctor(&self, project: &Path) -> Result<DoctorReport, AdapterError> {
        runtime::doctor(project, &self.capabilities(), self.external_command())
    }
}
