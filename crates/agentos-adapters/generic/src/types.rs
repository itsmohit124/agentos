//! Reports returned by adapter operations.

use serde::Serialize;

/// Whether an adapter operation is implemented.
///
/// `Optional` means the integration is real only when an external program is present.
/// `Unavailable` is reserved for a probe that looked and did not find that program.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Support {
    /// The adapter implements this operation locally.
    Supported,
    /// The adapter does not implement this operation.
    Unsupported,
    /// A probe found that an optional dependency is missing.
    Unavailable,
    /// The operation depends on an optional external program.
    Optional,
}

/// What this adapter actually implements.
///
/// Values are static. A missing external binary is reported by [`crate::DoctorReport`],
/// not by flipping a supported operation to available.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AdapterCapabilities {
    /// Adapter id (`generic`, `cursor`, `claude`, or `codex`).
    pub id: String,
    /// Install and uninstall of managed project files.
    pub install: Support,
    /// Skill discovery through the AgentOS registry.
    pub skills: Support,
    /// Context Packs from the context engine.
    pub context: Support,
    /// Workflow plans from the workflow engine.
    pub workflow: Support,
    /// Feedback through the feedback engine.
    pub feedback: Support,
    /// A specific external agent program. Generic has none.
    pub external_agent: Support,
}

/// What happened to one project-relative file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FileActionKind {
    /// The file did not exist and now contains AgentOS content.
    Created,
    /// AgentOS content in the file changed.
    Updated,
    /// A second install wrote the same bytes.
    Unchanged,
    /// A user-owned or user-modified file was left in place.
    Retained,
    /// AgentOS-managed content was removed.
    Removed,
    /// The file was left unchanged because changing it would not be safe.
    Failed,
}

/// One file touched or deliberately skipped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileAction {
    /// Project-relative path using `/`.
    pub path: String,
    /// What the operation did.
    pub action: FileActionKind,
    /// Short reason. Empty when the action is enough.
    pub detail: String,
}

/// Result of `install`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InstallReport {
    /// Adapter id.
    pub adapter: String,
    /// File actions, sorted by path.
    pub files: Vec<FileAction>,
}

/// Result of `uninstall`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UninstallReport {
    /// Adapter id.
    pub adapter: String,
    /// File actions, sorted by path.
    pub files: Vec<FileAction>,
}

impl UninstallReport {
    /// True when every file was removed, retained, or already absent.
    pub fn is_ok(&self) -> bool {
        !self
            .files
            .iter()
            .any(|file| file.action == FileActionKind::Failed)
    }
}

/// One skill as the registry exposes it. The body is not included.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExposedSkill {
    /// Skill name.
    pub name: String,
    /// Discovery description.
    pub description: String,
    /// Semantic version.
    pub version: String,
    /// `skill` or `router`.
    pub kind: String,
    /// The human may request it.
    pub user_invocable: bool,
    /// The model may choose it.
    pub model_invocable: bool,
    /// Declared capabilities, sorted.
    pub permissions: Vec<String>,
    /// Required skill names, in manifest order.
    pub requires: Vec<String>,
    /// Recommended companions, in manifest order.
    pub optional_recommends: Vec<String>,
    /// Abstract tool names, in manifest order.
    pub tools: Vec<String>,
    /// Discovery level (`project`, `agentos`, `builtin`).
    pub level: String,
    /// Display path of the canonical `SKILL.md`.
    pub path: String,
}

/// Skills an adapter may describe. This is the registry projection, not a copy of the bodies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SkillExposure {
    /// Skills sorted by name.
    pub skills: Vec<ExposedSkill>,
    /// Discovery diagnostics, sorted.
    pub diagnostics: Vec<String>,
}

/// A workflow plan produced by the existing planner.
///
/// The adapter does not start a run and does not claim the workflow succeeded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WorkflowInvocation {
    /// Selected workflow id.
    pub workflow: String,
    /// Plan id from the planner.
    pub plan_id: String,
    /// Why that workflow was selected.
    pub selection: String,
    /// Included step ids, in plan order.
    pub steps: Vec<String>,
    /// Skill names on the plan, sorted.
    pub skills: Vec<String>,
}

/// A request to forward an outcome into the feedback engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultRequest {
    /// Validation kind wire name, or `architecture`.
    pub kind: String,
    /// Caller task id. Stored on a validation result when the engine accepts one.
    pub task_id: Option<String>,
    /// Caller Context Pack id.
    pub context_pack_id: Option<String>,
    /// When set, the request is rejected. A claim is not a result.
    pub claimed_outcome: Option<String>,
    /// Capabilities the caller presents. Checked with `agentos_permissions::check`.
    pub permissions: Vec<String>,
}

/// What feedback actually returned.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReportedResult {
    /// Kind that was requested.
    pub kind: String,
    /// True only when a provider or the architecture analyzer ran.
    pub executed: bool,
    /// `passed`, `failed`, `refused`, `unavailable`, or another engine outcome.
    pub outcome: String,
    /// True only when every executed result passed.
    pub passed: bool,
    /// Counts and provider ids. No raw process log.
    pub detail: String,
    /// Provenance source (`architecture`, `feedback`, or a provider id).
    pub source: String,
    /// How the record was obtained.
    pub method: String,
}

/// One doctor check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DoctorCheck {
    /// Stable check name.
    pub name: String,
    /// `pass`, `info`, `warn`, or `fail`.
    pub status: String,
    /// What was observed.
    pub detail: String,
}

/// Doctor output. `ok` is false when any check is `fail`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DoctorReport {
    /// Adapter id.
    pub adapter: String,
    /// True when no check failed.
    pub ok: bool,
    /// Checks in display order.
    pub checks: Vec<DoctorCheck>,
}

/// One file an adapter wants installed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedWrite {
    /// Project-relative path using `/`.
    pub path: String,
    /// How the file is owned.
    pub policy: WritePolicy,
}

/// Ownership policy for one generated file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WritePolicy {
    /// Markdown (or Markdown-with-frontmatter) using `<!-- agentos:begin -->` markers.
    ///
    /// `prelude` is written only when AgentOS creates the file. An existing file keeps
    /// every byte outside the managed block.
    Markers {
        /// Bytes placed before the block when the file is created. Empty for a section
        /// appended to a user file. Must be valid for the format (frontmatter stays first).
        prelude: String,
        /// Interior of the managed block.
        body: String,
    },
    /// The entire file is AgentOS-owned. Used when HTML comments would make the format invalid.
    Whole {
        /// Full file contents, including a trailing newline.
        contents: String,
    },
}
