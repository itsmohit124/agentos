//! Read-only view of the project knowledge graph.
//!
//! The context engine never writes nodes. [`MemoryView`] is the fake store used by contract
//! tests. [`SqliteView`] reads the existing SQLite index, including domain tables.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use agentos_knowledge::{Finding, KnowledgeEdge, KnowledgeFragment, KnowledgeNode, KnowledgeStore};
use sha2::{Digest, Sha256};

use crate::error::ContextError;

/// What the retriever is allowed to see. Implementations must be deterministic.
pub trait KnowledgeView {
    /// Software and domain nodes, sorted by id.
    fn nodes(&self) -> Result<Vec<KnowledgeNode>, ContextError>;
    /// Software and domain edges, sorted by id.
    fn edges(&self) -> Result<Vec<KnowledgeEdge>, ContextError>;
    /// Fragments, sorted by id.
    fn fragments(&self) -> Result<Vec<KnowledgeFragment>, ContextError>;
    /// Domain findings. Ambiguity is evidence, not a merge.
    fn findings(&self) -> Result<Vec<Finding>, ContextError>;
    /// Stable fingerprint of the knowledge this view exposes.
    fn fingerprint(&self) -> Result<String, ContextError>;
    /// Optional lexical index. The default is empty; the engine still scans fragment text.
    fn fts(&self, query: &str) -> Result<Vec<String>, ContextError> {
        let _ = query;
        Ok(Vec::new())
    }
}

/// In-memory graph for tests and callers that already hold the records.
#[derive(Debug, Clone)]
pub struct MemoryView {
    /// Nodes.
    pub nodes: Vec<KnowledgeNode>,
    /// Edges.
    pub edges: Vec<KnowledgeEdge>,
    /// Fragments.
    pub fragments: Vec<KnowledgeFragment>,
    /// Findings.
    pub findings: Vec<Finding>,
    /// Caller-supplied fingerprint. Changing it invalidates a cache key.
    pub fingerprint: String,
    /// Fragment ids returned by [`KnowledgeView::fts`], keyed by the raw query.
    pub fts: BTreeMap<String, Vec<String>>,
    /// Times [`KnowledgeView::nodes`] was called. Cache hits must not increment it.
    pub node_reads: Arc<AtomicUsize>,
}

impl Default for MemoryView {
    fn default() -> Self {
        Self {
            nodes: Vec::new(),
            edges: Vec::new(),
            fragments: Vec::new(),
            findings: Vec::new(),
            fingerprint: "memory".to_owned(),
            fts: BTreeMap::new(),
            node_reads: Arc::new(AtomicUsize::new(0)),
        }
    }
}

impl KnowledgeView for MemoryView {
    fn nodes(&self) -> Result<Vec<KnowledgeNode>, ContextError> {
        self.node_reads.fetch_add(1, Ordering::SeqCst);
        let mut nodes = self.nodes.clone();
        nodes.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(nodes)
    }

    fn edges(&self) -> Result<Vec<KnowledgeEdge>, ContextError> {
        let mut edges = self.edges.clone();
        edges.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(edges)
    }

    fn fragments(&self) -> Result<Vec<KnowledgeFragment>, ContextError> {
        let mut fragments = self.fragments.clone();
        fragments.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(fragments)
    }

    fn findings(&self) -> Result<Vec<Finding>, ContextError> {
        let mut findings = self.findings.clone();
        findings.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(findings)
    }

    fn fingerprint(&self) -> Result<String, ContextError> {
        Ok(self.fingerprint.clone())
    }

    fn fts(&self, query: &str) -> Result<Vec<String>, ContextError> {
        Ok(self.fts.get(query).cloned().unwrap_or_default())
    }
}

/// SQLite-backed view of one project index.
pub struct SqliteView<'a> {
    store: &'a KnowledgeStore,
}

impl<'a> SqliteView<'a> {
    /// Borrows an open store. The caller owns scanning.
    pub fn new(store: &'a KnowledgeStore) -> Self {
        Self { store }
    }
}

impl KnowledgeView for SqliteView<'_> {
    fn nodes(&self) -> Result<Vec<KnowledgeNode>, ContextError> {
        Ok(self.store.nodes()?)
    }

    fn edges(&self) -> Result<Vec<KnowledgeEdge>, ContextError> {
        Ok(self.store.edges()?)
    }

    fn fragments(&self) -> Result<Vec<KnowledgeFragment>, ContextError> {
        Ok(self.store.fragments()?)
    }

    fn findings(&self) -> Result<Vec<Finding>, ContextError> {
        Ok(self.store.domain_findings()?)
    }

    fn fingerprint(&self) -> Result<String, ContextError> {
        let nodes = self.store.nodes()?;
        let edges = self.store.edges()?;
        let fragments = self.store.fragments()?;
        let files = self.store.fingerprints()?;
        Ok(content_fingerprint(&nodes, &edges, &fragments, &files))
    }

    fn fts(&self, query: &str) -> Result<Vec<String>, ContextError> {
        let mut ids = BTreeMap::<String, ()>::new();
        for token in crate::tokens::query_tokens(query) {
            if let Ok(hits) = self.store.search(&token) {
                for hit in hits {
                    ids.insert(hit, ());
                }
            }
        }
        Ok(ids.into_keys().collect())
    }
}

/// SHA-256 of the graph contents. Status is included so a confirm changes the key.
pub fn content_fingerprint(
    nodes: &[KnowledgeNode],
    edges: &[KnowledgeEdge],
    fragments: &[KnowledgeFragment],
    files: &BTreeMap<String, String>,
) -> String {
    let mut lines = Vec::new();
    for node in nodes {
        lines.push(format!(
            "n\t{}\t{}\t{}\t{}",
            node.id, node.fingerprint, node.status, node.confidence_millis
        ));
    }
    for edge in edges {
        lines.push(format!(
            "e\t{}\t{}\t{}\t{}",
            edge.id,
            edge.relation.as_str(),
            edge.from_id,
            edge.to_id
        ));
    }
    for fragment in fragments {
        lines.push(format!("f\t{}\t{}", fragment.id, fragment.fingerprint));
    }
    for (path, fingerprint) in files {
        lines.push(format!("s\t{path}\t{fingerprint}"));
    }
    lines.sort();
    let mut hasher = Sha256::new();
    for line in lines {
        hasher.update(line.as_bytes());
        hasher.update(b"\n");
    }
    hex_encode(&hasher.finalize())
}

pub(crate) fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}
