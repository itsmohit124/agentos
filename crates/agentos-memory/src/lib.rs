//! AgentOS memory: episodic, semantic, procedural, decision, and lesson records.
//!
//! Files in `.agentos/memory/` are the source of truth. SQLite rows mirror them and carry
//! the same provenance model as the knowledge graph. New records are `proposed` unless the
//! caller confirms them. Retrieval goes through the Context Engine. This crate does not
//! rank context on its own, and it does not call a model or the network.

mod clock;
mod error;
mod model;
mod record;
mod redact;
mod store;

pub use clock::{format_unix, utc_now};
pub use error::MemoryError;
pub use model::{
    FingerprintIndex, MEMORY_TYPES, MemoryLink, MemoryRecord, NewMemory, REVIEW_STATUSES,
};
pub use record::{
    RunCapture, StructuredSummarizer, Summarizer, SummaryInput, record_pattern, record_run,
    record_run_with,
};
pub use redact::{PATTERNS, redact};
pub use store::{MemoryStore, memory_id};

/// Name of this crate.
pub const CRATE_NAME: &str = env!("CARGO_PKG_NAME");

#[cfg(test)]
mod tests;
