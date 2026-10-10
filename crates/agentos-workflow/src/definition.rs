//! Shipped workflow definitions.
//!
//! `feature`, `bugfix`, and `refactor` are compiled into the binary. A step with
//! `skip_reason` is kept on the plan and is not executed.

use serde::Deserialize;

use crate::error::WorkflowError;

const FEATURE: &str = include_str!("../workflows/feature.yaml");
const BUGFIX: &str = include_str!("../workflows/bugfix.yaml");
const REFACTOR: &str = include_str!("../workflows/refactor.yaml");

/// One step in a workflow definition.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct StepSpec {
    /// Stable step id.
    pub id: String,
    /// What the step is for. Empty becomes the id.
    #[serde(default)]
    pub summary: String,
    /// Skill selected for this step.
    #[serde(default)]
    pub skill: Option<String>,
    /// Steps that must pass before this one.
    #[serde(default)]
    pub depends_on: Vec<String>,
    /// Output names taken from direct dependencies. Empty means every output of those steps.
    #[serde(default)]
    pub consumes: Vec<String>,
    /// Output names this step produces.
    #[serde(default)]
    pub outputs: Vec<String>,
    /// Phase 5 validation kind, when this step validates.
    #[serde(default)]
    pub validation: Option<String>,
    /// Capabilities the step itself declares, in addition to its skills.
    #[serde(default)]
    pub permissions: Vec<String>,
    /// Whether a failed validation may enter the fix loop.
    #[serde(default)]
    pub fix_loop: bool,
    /// When set, the step is skipped and this text is the reason.
    #[serde(default)]
    pub skip_reason: Option<String>,
}

impl StepSpec {
    /// A step with no skill, validation, or dependencies.
    pub fn new(id: impl Into<String>) -> Self {
        let id = id.into();
        Self {
            summary: id.clone(),
            id,
            skill: None,
            depends_on: Vec::new(),
            consumes: Vec::new(),
            outputs: Vec::new(),
            validation: None,
            permissions: Vec::new(),
            fix_loop: false,
            skip_reason: None,
        }
    }

    /// Sets direct dependencies.
    pub fn depends_on(mut self, ids: &[&str]) -> Self {
        self.depends_on = ids.iter().map(|id| (*id).to_owned()).collect();
        self
    }

    /// Sets the validation kind and enables the fix loop when `fix` is true.
    pub fn with_validation(mut self, kind: &str, fix: bool) -> Self {
        self.validation = Some(kind.to_owned());
        self.fix_loop = fix;
        self
    }

    /// Declares capabilities on the step.
    pub fn with_permissions(mut self, caps: &[&str]) -> Self {
        self.permissions = caps.iter().map(|cap| (*cap).to_owned()).collect();
        self
    }

    /// Names the skill the step selects.
    pub fn with_skill(mut self, skill: &str) -> Self {
        self.skill = Some(skill.to_owned());
        self
    }

    /// Marks the step skipped.
    pub fn skip(mut self, reason: &str) -> Self {
        self.skip_reason = Some(reason.to_owned());
        self
    }
}

/// A named workflow.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct WorkflowDefinition {
    /// Workflow id (`feature`, `bugfix`, `refactor`, or a test name).
    pub id: String,
    /// Task type this definition is for.
    pub task_type: String,
    /// What the workflow is for.
    pub description: String,
    /// Maximum steps admitted from one ready frontier.
    pub concurrency: u32,
    /// Maximum validation attempts in a fix loop.
    pub max_fix_iterations: u32,
    /// Steps in definition order.
    pub steps: Vec<StepSpec>,
}

impl WorkflowDefinition {
    /// A test definition. Concurrency defaults to 4 and the fix cap to 3.
    pub fn named(id: impl Into<String>, steps: Vec<StepSpec>) -> Self {
        let id = id.into();
        Self {
            task_type: id.clone(),
            description: id.clone(),
            id,
            concurrency: 4,
            max_fix_iterations: 3,
            steps,
        }
    }

    /// Parses a workflow YAML document.
    pub fn from_yaml(text: &str) -> Result<Self, WorkflowError> {
        let mut definition: Self = serde_norway::from_str(text)
            .map_err(|err| WorkflowError::Definition(err.to_string()))?;
        if definition.concurrency == 0 {
            return Err(WorkflowError::UnboundedConcurrency);
        }
        for step in &mut definition.steps {
            if step.summary.is_empty() {
                step.summary = step.id.clone();
            }
            step.depends_on.sort();
            step.depends_on.dedup();
            step.consumes.sort();
            step.consumes.dedup();
            step.outputs.sort();
            step.outputs.dedup();
            step.permissions.sort();
            step.permissions.dedup();
        }
        Ok(definition)
    }
}

/// The shipped workflow with `name`.
pub fn load(name: &str) -> Result<WorkflowDefinition, WorkflowError> {
    let text = match name {
        "feature" => FEATURE,
        "bugfix" => BUGFIX,
        "refactor" => REFACTOR,
        _ => return Err(WorkflowError::UnknownWorkflow(name.to_owned())),
    };
    WorkflowDefinition::from_yaml(text)
}

/// Shipped workflow names, sorted.
pub fn names() -> &'static [&'static str] {
    &["bugfix", "feature", "refactor"]
}
