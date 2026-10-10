//! Workflow engine: plan, schedule, and resume deterministic runs.
//!
//! Skill dependencies are expanded by the Phase 1 resolver. Each step's Context Pack
//! comes from the Phase 4 engine. Validation results come from Phase 5. This crate
//! does not parse tool output, call a model, or talk to a network.

mod context;
mod definition;
mod error;
mod execute;
mod graph;
mod plan;
mod run;
mod runner;
mod state;
mod store;
mod validation;
mod weaken;

pub use context::{ContextSource, EngineContext, StepContext, step_task};
pub use definition::{StepSpec, WorkflowDefinition, load, names};
pub use error::WorkflowError;
pub use execute::{Session, abort, resume, start, status};
pub use graph::SkippedStep;
pub use plan::{
    Edge, ExecutionMeta, Plan, PlanInput, PlannedStep, SelectedSkill, Selection, SkillEdge,
    ValidationRequirement, plan,
};
pub use run::{RunRecord, StepRun, normalize_for_resume};
pub use runner::{
    AgentRunner, ManualRunner, MockReaction, MockRunner, RunMode, RunnerResult, StepRequest,
};
pub use state::{RunState, Scope, Transition, allows};
pub use store::{load_plan, save_plan};
pub use validation::{
    PermissionCheck, Phase5Validator, ScriptedValidator, ValidationRequest, Validator,
    feedback_text, from_architecture, rejected, scripted_result,
};
pub use weaken::{TestEdit, WeakenFinding, detect};

#[cfg(test)]
mod tests;

/// Name of this crate.
pub const CRATE_NAME: &str = env!("CARGO_PKG_NAME");
