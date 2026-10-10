//! Typed workflow errors.

use agentos_skills::ResolveError;

/// Why planning or a run could not continue.
#[derive(Debug, thiserror::Error)]
pub enum WorkflowError {
    /// The workflow name is not one of the shipped definitions.
    #[error("unknown workflow {0:?} (known: bugfix, feature, refactor)")]
    UnknownWorkflow(String),
    /// A shipped or supplied definition is inconsistent.
    #[error("invalid workflow definition: {0}")]
    Definition(String),
    /// The task graph contains a cycle.
    #[error("workflow cycle: {}", .path.join(" -> "))]
    Cycle {
        /// The cycle, starting and ending on the same step.
        path: Vec<String>,
    },
    /// Phase 1 skill resolution failed.
    #[error(transparent)]
    Skills(#[from] ResolveError),
    /// A run or plan file is missing.
    #[error("{kind} {id:?} was not found")]
    NotFound {
        /// `plan` or `run`.
        kind: &'static str,
        /// The missing id.
        id: String,
    },
    /// The transition is not in the legal table.
    #[error("illegal transition: {scope} {id} {from} -> {to}")]
    IllegalTransition {
        /// `run` or `step`.
        scope: &'static str,
        /// Run or step id.
        id: String,
        /// State before the attempt.
        from: String,
        /// State that was rejected.
        to: String,
    },
    /// Resume was asked to continue a terminal run.
    #[error("run {id} is {state} and cannot be resumed")]
    NotResumable {
        /// Run id.
        id: String,
        /// Current state.
        state: String,
    },
    /// The manual runner has no result for this step.
    #[error("manual result missing for step {0}")]
    ManualResult(String),
    /// The context engine could not build a pack.
    #[error("context: {0}")]
    Context(String),
    /// Phase 5 validation could not be asked.
    #[error("validation: {0}")]
    Validation(String),
    /// A run file could not be read or written.
    #[error("run store: {0}")]
    Store(String),
    /// Concurrency was set to zero.
    #[error("concurrency must be at least 1")]
    UnboundedConcurrency,
}
