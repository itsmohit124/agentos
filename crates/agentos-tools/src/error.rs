//! Errors from the tool runtime.

use agentos_permissions::Capability;

/// Why a tool request did not run.
#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    /// The subject did not declare the required capability. Nothing was started.
    #[error("{reason}")]
    Denied {
        /// The permission decision reason.
        reason: String,
        /// The capability that was required.
        capability: Capability,
    },
    /// The artifact path is not inside `.agentos/runs/<id>/artifacts/`.
    #[error("artifact path is outside the run directory: {0}")]
    Artifact(String),
    /// The process could not be watched after it was spawned.
    #[error("cannot watch process: {0}")]
    Io(#[from] std::io::Error),
}
