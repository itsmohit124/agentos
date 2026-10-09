//! Optional semantic retrieval.
//!
//! Phase 4 ships no embedding model. [`NullSemantic`] returns no hits, and lexical plus
//! graph retrieval still produce a Context Pack.

use agentos_knowledge::KnowledgeNode;

/// One semantic hit. `score` is an arbitrary deterministic weight supplied by the retriever.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticHit {
    /// Logical node id.
    pub node_id: String,
    /// Non-negative score. Higher is more relevant.
    pub score: u32,
}

/// Semantic search behind a trait. The default engine does not require an implementation.
pub trait SemanticRetriever {
    /// Returns hits for `query`. An empty vec means this retriever adds nothing.
    fn search(&self, query: &str, nodes: &[KnowledgeNode]) -> Vec<SemanticHit>;
}

/// Retriever that never returns hits.
#[derive(Debug, Default, Clone, Copy)]
pub struct NullSemantic;

impl SemanticRetriever for NullSemantic {
    fn search(&self, _query: &str, _nodes: &[KnowledgeNode]) -> Vec<SemanticHit> {
        Vec::new()
    }
}
