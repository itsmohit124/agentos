//! AgentOS context engine: budgeted, traceable Context Packs.
//!
//! A pack is a projection of the existing project knowledge graph. It stores logical ids and
//! provenance sources. It does not create a second graph, and it does not upgrade Inferred or
//! Proposed domain knowledge.
//!
//! ```text
//! build(task) -> ContextPack
//! ```
//!
//! Lexical and graph retrieval work with no embedding model. [`NullSemantic`] is the default.

mod budget;
mod cache;
mod engine;
mod error;
mod memory;
mod pack;
mod render;
mod semantic;
mod tokens;
mod view;

pub use engine::{
    ContextConfig, build, build_with_memory, render_json, render_json_with_memory, stats, to_json,
};
pub use error::ContextError;
pub use memory::MemoryCandidate;
pub use pack::{
    Budget, ConstraintLink, ContextPack, ContextStats, Exclusion, Intent, RefItem, RetrievalTrace,
    ScoreParts, SelectedFragment, SelectedNode, TermHit, TerminologyEntry, TraceEntry,
};
pub use render::{explain, to_markdown};
pub use semantic::{NullSemantic, SemanticHit, SemanticRetriever};
pub use tokens::intent_kind;
pub use view::{KnowledgeView, MemoryView, SqliteView, content_fingerprint};

/// Name of this crate.
pub const CRATE_NAME: &str = env!("CARGO_PKG_NAME");

#[cfg(test)]
mod tests;
