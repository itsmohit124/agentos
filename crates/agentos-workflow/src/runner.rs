//! Agent runners. Tests use [`MockRunner`]. A human session uses [`ManualRunner`].
//!
//! Neither runner calls Cursor, Claude, or Codex.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde::{Deserialize, Serialize};

use crate::error::WorkflowError;
use crate::weaken::TestEdit;

/// Whether the runner is carrying out the step or repairing a failed validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunMode {
    /// The step's own work.
    Execute,
    /// A bounded repair after failed validation.
    Fix,
}

impl RunMode {
    /// Wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Execute => "execute",
            Self::Fix => "fix",
        }
    }
}

/// What one step asks a runner to do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepRequest {
    /// Step id.
    pub step_id: String,
    /// Execute or fix.
    pub mode: RunMode,
    /// The user task, unchanged.
    pub task: String,
    /// Instructions for the step.
    pub instructions: String,
    /// Output names the step is expected to produce.
    pub outputs: Vec<String>,
    /// Context Pack id.
    pub context_pack_id: String,
    /// Task text inside the Context Pack.
    pub context_task: String,
    /// Outputs of declared dependencies, keyed `step.output`.
    pub dependency_outputs: BTreeMap<String, String>,
    /// Fragment ids selected for this step.
    pub selected_fragment_ids: Vec<String>,
}

/// What a runner finished.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunnerResult {
    /// True when the step work failed.
    pub failed: bool,
    /// Short result text.
    pub message: String,
    /// Named outputs.
    pub outputs: BTreeMap<String, String>,
    /// Test edits made by a fix. The engine detects weakening from these pairs.
    pub edits: Vec<TestEdit>,
}

impl RunnerResult {
    /// A successful result with no outputs.
    pub fn success(message: impl Into<String>) -> Self {
        Self {
            failed: false,
            message: message.into(),
            outputs: BTreeMap::new(),
            edits: Vec::new(),
        }
    }
}

/// Executes one step. Implementations must not call a model or the network.
pub trait AgentRunner {
    /// Runs `request`.
    fn run(&mut self, request: &StepRequest) -> Result<RunnerResult, WorkflowError>;

    /// True when the runner cannot proceed until a result is supplied.
    fn needs_input(&self, _step_id: &str) -> bool {
        false
    }

    /// True when the session should stop after the step that just finished.
    fn suspended(&self) -> bool {
        false
    }

    /// Clears session flags at the start of `start` or `resume`.
    fn begin_session(&mut self) {}

    /// Records a request that is shown to the operator before a result exists.
    fn observe(&mut self, _request: &StepRequest) {}
}

/// One scripted reaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MockReaction {
    /// When true, the step work fails.
    pub failed: bool,
    /// Result message.
    pub message: String,
    /// Named outputs. Empty uses `{name}:ok` for each requested output.
    pub outputs: BTreeMap<String, String>,
    /// Test edits.
    pub edits: Vec<TestEdit>,
}

impl MockReaction {
    /// A passing reaction.
    pub fn ok() -> Self {
        Self {
            failed: false,
            message: "ok".to_owned(),
            outputs: BTreeMap::new(),
            edits: Vec::new(),
        }
    }

    /// A failing reaction.
    pub fn fail(message: impl Into<String>) -> Self {
        Self {
            failed: true,
            message: message.into(),
            outputs: BTreeMap::new(),
            edits: Vec::new(),
        }
    }

    /// A passing fix that reports test edits.
    pub fn edits(edits: Vec<TestEdit>) -> Self {
        Self {
            failed: false,
            message: "edited".to_owned(),
            outputs: BTreeMap::new(),
            edits,
        }
    }
}

/// Deterministic scripted runner used by workflow tests.
#[derive(Debug, Default)]
pub struct MockRunner {
    scripts: BTreeMap<String, VecDeque<MockReaction>>,
    suspend_after: BTreeSet<String>,
    suspend_flag: bool,
    /// `step:mode` in call order.
    pub calls: Vec<String>,
}

impl MockRunner {
    /// Succeeds every step and fills each declared output with `{name}:ok`.
    pub fn succeed() -> Self {
        Self::default()
    }

    /// Queues reactions for `step_id`. Later calls use the default success.
    pub fn script(mut self, step_id: &str, reactions: Vec<MockReaction>) -> Self {
        self.scripts
            .insert(step_id.to_owned(), VecDeque::from(reactions));
        self
    }

    /// After `step_id` returns, the engine persists it and stops the session.
    pub fn suspend_after(mut self, step_id: &str) -> Self {
        self.suspend_after.insert(step_id.to_owned());
        self
    }
}

impl AgentRunner for MockRunner {
    fn run(&mut self, request: &StepRequest) -> Result<RunnerResult, WorkflowError> {
        self.calls
            .push(format!("{}:{}", request.step_id, request.mode.as_str()));
        let reaction = self
            .scripts
            .get_mut(&request.step_id)
            .and_then(VecDeque::pop_front);
        let reaction = reaction.unwrap_or_else(MockReaction::ok);
        let mut outputs = reaction.outputs;
        if outputs.is_empty() && !reaction.failed {
            for name in &request.outputs {
                outputs.insert(name.clone(), format!("{name}:ok"));
            }
        }
        if self.suspend_after.contains(&request.step_id) && request.mode == RunMode::Execute {
            self.suspend_flag = true;
        }
        Ok(RunnerResult {
            failed: reaction.failed,
            message: reaction.message,
            outputs,
            edits: reaction.edits,
        })
    }

    fn suspended(&self) -> bool {
        self.suspend_flag
    }

    fn begin_session(&mut self) {
        self.suspend_flag = false;
    }
}

/// Prints the step instructions and Context Pack, then records a supplied result.
#[derive(Debug, Default)]
pub struct ManualRunner {
    queued: BTreeMap<String, VecDeque<RunnerResult>>,
    /// Requests that were emitted, in order.
    pub emissions: Vec<StepRequest>,
}

impl ManualRunner {
    /// A runner with no supplied results. The first step waits.
    pub fn new() -> Self {
        Self::default()
    }

    /// Supplies the next result for `step_id`.
    pub fn push(&mut self, step_id: &str, result: RunnerResult) {
        self.queued
            .entry(step_id.to_owned())
            .or_default()
            .push_back(result);
    }
}

impl AgentRunner for ManualRunner {
    fn run(&mut self, request: &StepRequest) -> Result<RunnerResult, WorkflowError> {
        self.emissions.push(request.clone());
        self.queued
            .get_mut(&request.step_id)
            .and_then(VecDeque::pop_front)
            .ok_or_else(|| WorkflowError::ManualResult(request.step_id.clone()))
    }

    fn needs_input(&self, step_id: &str) -> bool {
        self.queued.get(step_id).is_none_or(VecDeque::is_empty)
    }

    fn observe(&mut self, request: &StepRequest) {
        self.emissions.push(request.clone());
    }
}
