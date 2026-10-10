//! Errors from the tool runtime.

use agentos_permissions::Capability;

/// Why a tool request did not run.
#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    /// The subject did not declare `shell`. Nothing was spawned.
    #[error("{reason}")]
    Denied {
        /// The permission decision reason.
        reason: String,
        /// The capability that was required.
        capability: Capability,
    },
    /// The process could not be watched after it was spawned.
    #[error("cannot watch process: {0}")]
    Io(#[from] std::io::Error),
}
