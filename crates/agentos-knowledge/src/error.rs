//! Errors from project understanding and the knowledge store.

use std::path::PathBuf;

/// Failure while reading a project, extracting symbols, or writing the index.
#[derive(Debug, thiserror::Error)]
pub enum KnowledgeError {
    /// The database could not be opened or migrated.
    #[error(transparent)]
    Db(#[from] agentos_core::db::DbError),
    /// A filesystem operation failed.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// SQLite rejected a statement.
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    /// Two different files produced the same logical id.
    #[error("duplicate logical id {id} from {first} and {second}")]
    DuplicateId {
        /// The id both files claimed.
        id: String,
        /// First relative path.
        first: String,
        /// Second relative path.
        second: String,
    },
    /// `inspect` was asked for an id the index does not contain.
    #[error("no knowledge node {0}")]
    NotFound(String),
    /// A grammar failed to load or a file failed to parse into a tree.
    #[error("{language} extractor: {message}")]
    Extract {
        /// Extractor name (`python`, `typescript`, …).
        language: &'static str,
        /// What went wrong.
        message: String,
    },
    /// The persisted profile is not valid JSON of the expected shape.
    #[error("invalid project profile {}: {message}", path.display())]
    Profile {
        /// Profile path.
        path: PathBuf,
        /// What is wrong.
        message: String,
    },
    /// A domain lifecycle change is not one of the allowed steps.
    #[error("illegal domain transition from {from} to {to}")]
    IllegalTransition {
        /// Status before the change.
        from: String,
        /// Status that was refused.
        to: String,
    },
    /// A proposer tried to write Confirmed or Canonical knowledge.
    #[error("domain proposer {proposer} tried to write {status}")]
    ProposerForbidden {
        /// [`crate::domain::DomainProposer::name`].
        proposer: String,
        /// The status that was refused.
        status: String,
    },
    /// `confirm` or `reject` refused the requested change.
    #[error("cannot update domain node {id}: {reason}")]
    DomainUpdate {
        /// Logical id.
        id: String,
        /// Why the command stopped.
        reason: String,
    },
}
