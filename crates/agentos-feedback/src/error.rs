//! Errors from validation and architecture analysis.

use crate::kind::ValidationKind;

/// Why a feedback operation could not finish.
#[derive(Debug, thiserror::Error)]
pub enum FeedbackError {
    /// The override names a provider this build does not know.
    #[error("unknown provider {0}")]
    UnknownProvider(String),
    /// The provider cannot run that kind.
    #[error("provider {provider} does not support {kind}")]
    UnsupportedKind {
        /// Provider id.
        provider: String,
        /// Requested kind.
        kind: ValidationKind,
    },
    /// `.agentos/config.toml` has a validation or architecture value this engine rejects.
    #[error("invalid feedback config: {0}")]
    Config(String),
    /// `validate last` found no stored result.
    #[error("no validation result has been recorded")]
    NoResult,
    /// `record-regression` was given an id that is not in the index.
    #[error("unknown validation result {0}")]
    UnknownResult(String),
    /// A passing result is not a regression case.
    #[error("validation {0} passed; a regression records a failure")]
    NotAFailure(String),
    /// Refusal is a permission result. Replay parses process output, so it cannot reproduce one.
    #[error("validation {0} was refused; regression replay needs process output")]
    NotReplayable(String),
    /// The id is not safe to use as a file name.
    #[error("invalid validation id {0}")]
    InvalidId(String),
    /// The knowledge index could not be read or built.
    #[error(transparent)]
    Knowledge(#[from] agentos_knowledge::KnowledgeError),
    /// A feedback file could not be read or written.
    #[error("cannot store feedback: {0}")]
    Io(#[from] std::io::Error),
    /// The tool runtime failed while watching a process that had already been allowed.
    #[error(transparent)]
    Tool(#[from] agentos_tools::ToolError),
}
