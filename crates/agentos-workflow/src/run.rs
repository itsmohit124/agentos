//! Persisted run records.
//!
//! A run keeps the step metadata it needs to resume. Completed steps stay `passed`.

use std::collections::BTreeMap;

use agentos_context::ContextPack;
use agentos_feedback::ValidationResult;
use serde::{Deserialize, Serialize};

use crate::error::WorkflowError;
use crate::graph::SkippedStep;
use crate::plan::Plan;
use crate::state::{self, RunState, Scope, Transition};
use crate::validation::PermissionCheck;

/// One step inside a run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepRun {
    /// Step id.
    pub id: String,
    /// Summary.
    pub summary: String,
    /// Skill named by the step.
    pub skill: Option<String>,
    /// Included dependencies.
    pub depends_on: Vec<String>,
    /// Output names consumed from those dependencies.
    pub consumes: Vec<String>,
    /// Output names this step produces.
    pub outputs: Vec<String>,
    /// Validation kind.
    pub validation: Option<String>,
    /// Fix loop enabled.
    pub fix_loop: bool,
    /// Capabilities the step declares.
    pub granted: Vec<String>,
    /// Capabilities required before execution.
    pub required: Vec<String>,
    /// Current state.
    pub state: RunState,
    /// Outputs produced by the runner.
    pub outputs_produced: BTreeMap<String, String>,
    /// Context Pack id.
    pub context_pack_id: Option<String>,
    /// Task text of that pack.
    pub context_task: Option<String>,
    /// Fragment ids in the pack.
    pub selected_fragment_ids: Vec<String>,
    /// Dependency outputs passed into the step.
    pub dependency_outputs: BTreeMap<String, String>,
    /// The Context Pack, when one was built.
    pub context_pack: Option<ContextPack>,
    /// Permission decisions for this attempt.
    pub permission_checks: Vec<PermissionCheck>,
    /// Validation records, one per attempt.
    pub validations: Vec<ValidationResult>,
    /// Validation attempts used.
    pub attempts: u32,
    /// Why the step failed.
    pub failure_reason: Option<String>,
    /// Why the step was blocked.
    pub block_reason: Option<String>,
}

/// A resumable run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunRecord {
    /// Run id.
    pub id: String,
    /// Plan id.
    pub plan_id: String,
    /// User task, unchanged.
    pub task: String,
    /// Workflow id.
    pub workflow: String,
    /// Current state.
    pub state: RunState,
    /// Admission limit copied from the plan.
    pub concurrency: u32,
    /// Fix-loop cap copied from the plan.
    pub max_fix_iterations: u32,
    /// Included steps.
    pub steps: Vec<StepRun>,
    /// Skipped steps and their reasons.
    pub skipped: Vec<SkippedStep>,
    /// Every accepted transition.
    pub transitions: Vec<Transition>,
    /// Ready frontiers observed before each admission.
    pub frontiers: Vec<Vec<String>>,
    /// Steps admitted together, in start order.
    pub waves: Vec<Vec<String>>,
    /// `step:mode` calls that finished or started work.
    pub executed: Vec<String>,
    /// The session stopped after persisting completed steps.
    pub interrupted: bool,
    /// The manual runner is waiting for a result.
    pub waiting: bool,
    /// Step the manual runner is waiting on.
    pub pending_step: Option<String>,
    /// Instructions emitted for that step.
    pub pending_instructions: Option<String>,
    /// Why the run failed or was blocked.
    pub failure_reason: Option<String>,
}

impl RunRecord {
    /// A planned run for `plan`.
    pub fn from_plan(plan: &Plan, id: String) -> Self {
        Self {
            id,
            plan_id: plan.id.clone(),
            task: plan.task.clone(),
            workflow: plan.workflow.clone(),
            state: RunState::Planned,
            concurrency: plan.execution.concurrency,
            max_fix_iterations: plan.execution.max_fix_iterations,
            steps: plan
                .steps
                .iter()
                .map(|step| StepRun {
                    id: step.id.clone(),
                    summary: step.summary.clone(),
                    skill: step.skill.clone(),
                    depends_on: step.depends_on.clone(),
                    consumes: step.consumes.clone(),
                    outputs: step.outputs.clone(),
                    validation: step.validation.clone(),
                    fix_loop: step.fix_loop,
                    granted: step.granted.clone(),
                    required: step.required.clone(),
                    state: RunState::Planned,
                    outputs_produced: BTreeMap::new(),
                    context_pack_id: None,
                    context_task: None,
                    selected_fragment_ids: Vec::new(),
                    dependency_outputs: BTreeMap::new(),
                    context_pack: None,
                    permission_checks: Vec::new(),
                    validations: Vec::new(),
                    attempts: 0,
                    failure_reason: None,
                    block_reason: None,
                })
                .collect(),
            skipped: plan.skipped.clone(),
            transitions: Vec::new(),
            frontiers: Vec::new(),
            waves: Vec::new(),
            executed: Vec::new(),
            interrupted: false,
            waiting: false,
            pending_step: None,
            pending_instructions: None,
            failure_reason: None,
        }
    }

    /// Moves the run to `to` when that edge is legal.
    pub fn transition_run(&mut self, to: RunState) -> Result<(), WorkflowError> {
        let from = self.state;
        state::ensure(Scope::Run, &self.id, from, to)?;
        self.push_transition(Scope::Run, self.id.clone(), from, to);
        self.state = to;
        tracing::info!(run = %self.id, from = %from, to = %to, "run transition");
        Ok(())
    }

    /// Moves a step to `to` when that edge is legal.
    pub fn transition_step(&mut self, step_id: &str, to: RunState) -> Result<(), WorkflowError> {
        let index = self.step_index(step_id)?;
        let from = self.steps[index].state;
        state::ensure(Scope::Step, step_id, from, to)?;
        self.push_transition(Scope::Step, step_id.to_owned(), from, to);
        self.steps[index].state = to;
        tracing::info!(step = %step_id, from = %from, to = %to, "step transition");
        Ok(())
    }

    fn step_index(&self, step_id: &str) -> Result<usize, WorkflowError> {
        self.steps
            .iter()
            .position(|step| step.id == step_id)
            .ok_or_else(|| WorkflowError::Definition(format!("unknown step {step_id:?}")))
    }

    fn push_transition(&mut self, scope: Scope, id: String, from: RunState, to: RunState) {
        self.transitions.push(Transition {
            seq: u64::try_from(self.transitions.len()).unwrap_or(u64::MAX) + 1,
            scope: scope.as_str().to_owned(),
            id,
            from,
            to,
        });
    }
}

/// Returns steps left `running` by an interrupted session to `ready`.
pub fn normalize_for_resume(run: &mut RunRecord) -> Result<(), WorkflowError> {
    let running: Vec<String> = run
        .steps
        .iter()
        .filter(|step| step.state == RunState::Running)
        .map(|step| step.id.clone())
        .collect();
    for id in running {
        run.transition_step(&id, RunState::Ready)?;
    }
    Ok(())
}
