//! Context Pack records.
//!
//! A pack is a projection of the project knowledge graph. Selected items keep the logical
//! ids and provenance sources of the nodes they came from. Status strings are copied, never
//! upgraded.

use serde::{Deserialize, Serialize};

/// How a task was classified, and which terms routed it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Intent {
    /// `feature`, `bugfix`, `refactor`, `review`, or `question`.
    pub kind: String,
    /// The rule that fired, including the matched word when there is one.
    pub rule: String,
    /// Routed bounded-context ids, sorted.
    pub contexts: Vec<String>,
    /// Task term to node to context. Sorted by term, then node id.
    pub terms: Vec<TermHit>,
}

/// One ubiquitous-language match.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TermHit {
    /// Token from the task.
    pub task_term: String,
    /// Logical id of the matching node.
    pub node_id: String,
    /// Bounded context that match routed.
    pub context_id: String,
}

/// Token budget applied to this pack.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Budget {
    /// `None` means every relevant item was eligible. It does not mean the whole repository.
    pub limit: Option<u32>,
    /// Tokens kept.
    pub used: u32,
    /// Candidate pieces dropped because they did not fit.
    pub dropped: u32,
}

/// Score components recorded on a trace entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ScoreParts {
    /// Name or summary matched a task term.
    pub term: u32,
    /// The node sits in a routed bounded context.
    pub context: u32,
    /// Distinct task terms found in text.
    pub lexical: u32,
    /// An edge connects the node to a matched node.
    pub graph: u32,
    /// Score supplied by the semantic retriever. Zero when none is configured.
    pub semantic: u32,
    /// Sum of the components.
    pub total: u32,
}

/// A node kept in the pack.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectedNode {
    /// Logical id.
    pub id: String,
    /// Node kind wire name.
    pub node_type: String,
    /// Local name.
    pub name: String,
    /// Lifecycle or software status, copied from the graph.
    pub status: String,
    /// Budget priority. 1 is dropped last.
    pub priority: u8,
    /// Total score.
    pub score: u32,
    /// Relative provenance source.
    pub source: String,
    /// Rule that selected the node.
    pub reason: String,
    /// Edge that attached it, or empty.
    pub relationship: String,
    /// Token estimate of the node label.
    pub token_estimate: u32,
}

/// A fragment kept in the pack.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectedFragment {
    /// Fragment id.
    pub id: String,
    /// Owning logical node id.
    pub node_id: String,
    /// Relative source path.
    pub source_ref: String,
    /// Line span.
    pub location: String,
    /// Fragment kind (`code`, `import`, `section`).
    pub kind: String,
    /// Token estimate of `content`.
    pub token_estimate: u32,
    /// Exact slice text.
    pub content: String,
    /// Why the fragment was kept.
    pub reason: String,
}

/// A short reference to a selected node. The logical id is the link back to the graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RefItem {
    /// Logical id.
    pub id: String,
    /// Local name.
    pub name: String,
    /// Status copied from the graph.
    pub status: String,
    /// Relative provenance source.
    pub source: String,
    /// One-line summary from the graph.
    pub summary: String,
}

/// A dependency or call among selected nodes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConstraintLink {
    /// Source logical id.
    pub from_id: String,
    /// Relation wire name.
    pub relation: String,
    /// Target logical id.
    pub to_id: String,
    /// Relative provenance source.
    pub source: String,
}

/// A terminology node, still labelled with its lifecycle status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TerminologyEntry {
    /// Logical id.
    pub id: String,
    /// Term.
    pub name: String,
    /// Lifecycle status. Proposed stays `proposed`. Inferred stays `inferred`.
    pub status: String,
    /// Relative provenance source.
    pub source: String,
    /// Other term ids linked by `CONFLICTS_WITH`. Not a merge.
    pub conflicts: Vec<String>,
    /// Finding ids that name this term. Ambiguity is not resolved here.
    pub finding_ids: Vec<String>,
}

/// A node or fragment left out, with the reason.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Exclusion {
    /// Logical id or fragment id.
    pub id: String,
    /// `node` or `fragment`.
    pub item_kind: String,
    /// Why it was left out.
    pub reason: String,
    /// Edge that was considered, or empty.
    pub relationship: String,
    /// Relative provenance source.
    pub source: String,
    /// Token estimate.
    pub token_estimate: u32,
}

/// One selected or excluded item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TraceEntry {
    /// Logical id or fragment id.
    pub id: String,
    /// `node` or `fragment`.
    pub item_kind: String,
    /// `selected` or `excluded`.
    pub outcome: String,
    /// Rule that fired.
    pub reason: String,
    /// Edge that caused expansion, or empty.
    pub relationship: String,
    /// Relative provenance source.
    pub source: String,
    /// Token estimate.
    pub token_estimate: u32,
    /// Budget priority.
    pub priority: u8,
    /// Score components.
    pub score: ScoreParts,
}

/// Explainable retrieval decisions, sorted by id then item kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetrievalTrace {
    /// Every traced item.
    pub entries: Vec<TraceEntry>,
}

/// Projection of the knowledge graph for one task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextPack {
    /// Task text.
    pub task: String,
    /// Classification and routed terms.
    pub intent: Intent,
    /// Budget that was applied.
    pub budget: Budget,
    /// Kept nodes, sorted by id.
    pub selected_nodes: Vec<SelectedNode>,
    /// Kept fragments, sorted by id.
    pub selected_fragments: Vec<SelectedFragment>,
    /// Terminology in the pack.
    pub terminology: Vec<TerminologyEntry>,
    /// Modules and architecture documents.
    pub architecture: Vec<RefItem>,
    /// Imports and calls among selected nodes.
    pub constraints: Vec<ConstraintLink>,
    /// Invariants.
    pub invariants: Vec<RefItem>,
    /// ADRs and other decisions.
    pub decisions: Vec<RefItem>,
    /// Tests.
    pub tests: Vec<RefItem>,
    /// Left-out items.
    pub exclusions: Vec<Exclusion>,
    /// Why each item was kept or left out.
    pub retrieval_trace: RetrievalTrace,
}

/// Counts for `context stats`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextStats {
    /// Intent kind.
    pub intent: String,
    /// Routed context ids.
    pub contexts: Vec<String>,
    /// Kept nodes.
    pub selected_nodes: usize,
    /// Kept fragments.
    pub selected_fragments: usize,
    /// Tokens kept.
    pub tokens: u32,
    /// Budget limit.
    pub budget: Option<u32>,
    /// Excluded items.
    pub excluded: usize,
}

impl ContextStats {
    /// Summarises a pack.
    pub fn from_pack(pack: &ContextPack) -> Self {
        Self {
            intent: pack.intent.kind.clone(),
            contexts: pack.intent.contexts.clone(),
            selected_nodes: pack.selected_nodes.len(),
            selected_fragments: pack.selected_fragments.len(),
            tokens: pack.budget.used,
            budget: pack.budget.limit,
            excluded: pack.exclusions.len(),
        }
    }
}
