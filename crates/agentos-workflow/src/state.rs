//! Run states and the legal transition table.
//!
//! A step and a run share the state names. They do not share every edge.
//! `passed` is a successful step. `completed` is a successful run.
//! An illegal transition leaves the previous state unchanged.

use serde::{Deserialize, Serialize};

use crate::error::WorkflowError;

/// Observable state of a run or a step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunState {
    /// The plan exists and execution has not started.
    Planned,
    /// At least one step may run, and nothing is in flight.
    Ready,
    /// A step is in flight, or the run has unfinished work from a session.
    Running,
    /// The step succeeded, or every included step has passed.
    Passed,
    /// A step or the run failed.
    Failed,
    /// Permission was denied. The runner was not called.
    Blocked,
    /// The operator aborted the run.
    Aborted,
    /// The run finished after every included step passed.
    Completed,
}

impl RunState {
    /// Every state, in the contract order.
    pub const ALL: [Self; 8] = [
        Self::Planned,
        Self::Ready,
        Self::Running,
        Self::Passed,
        Self::Failed,
        Self::Blocked,
        Self::Aborted,
        Self::Completed,
    ];

    /// Wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Planned => "planned",
            Self::Ready => "ready",
            Self::Running => "running",
            Self::Passed => "passed",
            Self::Failed => "failed",
            Self::Blocked => "blocked",
            Self::Aborted => "aborted",
            Self::Completed => "completed",
        }
    }

    /// Whether execution must not continue from this state.
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Failed | Self::Blocked | Self::Aborted | Self::Completed
        )
    }
}

impl std::fmt::Display for RunState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Whether the edge belongs to a run or a step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    /// The run record.
    Run,
    /// One step.
    Step,
}

impl Scope {
    /// Wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Run => "run",
            Self::Step => "step",
        }
    }
}

/// One persisted transition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Transition {
    /// 1-based order inside the run.
    pub seq: u64,
    /// `run` or `step`.
    pub scope: String,
    /// Run id or step id.
    pub id: String,
    /// State before the edge.
    pub from: RunState,
    /// State after the edge.
    pub to: RunState,
}

/// Whether `from → to` is legal for `scope`.
pub fn allows(scope: Scope, from: RunState, to: RunState) -> bool {
    use RunState::{Aborted, Blocked, Completed, Failed, Passed, Planned, Ready, Running};
    matches!(
        (scope, from, to),
        (Scope::Step, Planned, Ready | Aborted)
            | (Scope::Step, Ready, Running | Blocked | Aborted)
            | (Scope::Step, Running, Passed | Failed | Aborted | Ready)
            | (Scope::Run, Planned, Ready | Aborted)
            | (Scope::Run, Ready, Running | Blocked | Aborted)
            | (
                Scope::Run,
                Running,
                Ready | Passed | Failed | Blocked | Aborted
            )
            | (Scope::Run, Passed, Completed)
    )
}

/// Rejects an illegal edge. The caller updates its state only after this returns `Ok`.
pub fn ensure(scope: Scope, id: &str, from: RunState, to: RunState) -> Result<(), WorkflowError> {
    if allows(scope, from, to) {
        Ok(())
    } else {
        Err(WorkflowError::IllegalTransition {
            scope: scope.as_str(),
            id: id.to_owned(),
            from: from.as_str().to_owned(),
            to: to.as_str().to_owned(),
        })
    }
}
