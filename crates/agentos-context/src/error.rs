//! Errors from context assembly. Retrieval does not call a model or the network.

use agentos_knowledge::KnowledgeError;

/// Failure while reading knowledge or the context cache.
#[derive(Debug, thiserror::Error)]
pub enum ContextError {
    /// The knowledge index could not be read.
    #[error(transparent)]
    Knowledge(#[from] KnowledgeError),
    /// The cache file could not be read or written.
    #[error("context cache: {0}")]
    Cache(String),
    /// JSON encoding or a cached pack could not be read.
    #[error("context pack json: {0}")]
    Json(#[from] serde_json::Error),
    /// Filesystem error while writing the cache.
    #[error(transparent)]
    Io(#[from] std::io::Error),
}
