//! Adapter failures. Filesystem and contract errors stay typed.

use std::path::Path;

/// Why an adapter operation stopped.
#[derive(Debug, thiserror::Error)]
pub enum AdapterError {
    /// The project path is not a directory.
    #[error("project {} is not a directory", .path.display())]
    NotAProject {
        /// The path that was supplied.
        path: std::path::PathBuf,
    },
    /// A managed path would leave the project root.
    #[error("path {path:?} escapes the project")]
    UnsafePath {
        /// The rejected relative path.
        path: String,
    },
    /// A managed file could not be changed safely.
    #[error("{path}: {message}")]
    Managed {
        /// Project-relative path.
        path: String,
        /// What is wrong.
        message: String,
    },
    /// Reading or writing a file failed.
    #[error("{path}: {message}")]
    Io {
        /// Path involved.
        path: String,
        /// Filesystem message.
        message: String,
    },
    /// The skill registry or an invocation check rejected the request.
    #[error("{0}")]
    Skills(String),
    /// Context assembly failed.
    #[error("context: {0}")]
    Context(String),
    /// The workflow planner rejected the request.
    #[error("workflow: {0}")]
    Workflow(String),
    /// Feedback execution failed.
    #[error("feedback: {0}")]
    Feedback(String),
    /// The caller asked to record an outcome that was not produced by AgentOS.
    #[error("unverified outcome claim {0:?}; an adapter does not record a pass without a result")]
    UnverifiedClaim(String),
    /// The outcome kind is not a feedback kind this adapter forwards.
    #[error("unsupported outcome {0:?}")]
    UnsupportedOutcome(String),
}

impl AdapterError {
    pub(crate) fn io(path: &Path, err: &std::io::Error) -> Self {
        Self::Io {
            path: path.display().to_string(),
            message: err.to_string(),
        }
    }

    pub(crate) fn managed(path: &str, message: impl Into<String>) -> Self {
        Self::Managed {
            path: path.to_owned(),
            message: message.into(),
        }
    }
}
