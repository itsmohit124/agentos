//! Errors from memory persistence. Nothing here contacts a network.

use std::path::PathBuf;

/// Failure while reading or writing project memory.
#[derive(Debug, thiserror::Error)]
pub enum MemoryError {
    /// No record has this id.
    #[error("unknown memory {0}")]
    NotFound(String),
    /// `confirm` was asked to revive a forgotten record.
    #[error("forgotten memory {0} cannot be confirmed")]
    Forgotten(String),
    /// The type is not one of the five memory types.
    #[error("invalid memory type {0}")]
    Type(String),
    /// A link is missing a kind, id, or fingerprint, or the kind is unknown.
    #[error("invalid memory link {0}")]
    Link(String),
    /// Name or content was empty after redaction.
    #[error("memory name and content must not be empty")]
    Empty,
    /// The caller did not supply a timestamp.
    #[error("recorded_at must not be empty")]
    Clock,
    /// A memory file could not be parsed.
    #[error("memory file {}: {message}", .path.display())]
    File {
        /// The file.
        path: PathBuf,
        /// What is wrong.
        message: String,
    },
    /// JSON encoding failed.
    #[error("memory json: {0}")]
    Json(#[from] serde_json::Error),
    /// The filesystem rejected a read or write.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// SQLite rejected a statement.
    #[error(transparent)]
    Db(#[from] rusqlite::Error),
    /// The project database could not be opened.
    #[error(transparent)]
    Core(#[from] agentos_core::db::DbError),
    /// The knowledge index could not be read while collecting fingerprints.
    #[error(transparent)]
    Knowledge(#[from] agentos_knowledge::KnowledgeError),
}
