//! Memory candidates inside the existing retrieval pipeline.
//!
//! A candidate is ranked, budgeted, and traced with the graph. An empty slice adds nothing,
//! so a project with no memory keeps the Phase 4 pack. Forgotten records are omitted here;
//! the store does not hand them over.

use std::collections::{BTreeSet, HashSet};

use agentos_knowledge::token_estimate;

use crate::budget::BudgetItem;
use crate::pack::{Exclusion, ScoreParts, SelectedFragment, SelectedNode, TraceEntry};
use crate::tokens::tokens;

/// One memory record the retriever may consider.
///
/// The store fills this. The engine does not read the memory tables itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryCandidate {
    /// Stable id (`memory.<type>.<hex>`).
    pub id: String,
    /// `episodic`, `semantic`, `procedural`, `decision`, or `lesson`.
    pub memory_type: String,
    /// Short name.
    pub name: String,
    /// `proposed`, `confirmed`, `forgotten`, or any other string.
    ///
    /// `canonical` is not honored. Forgotten records are dropped before ranking.
    pub status: String,
    /// Body that may be placed in a fragment.
    pub content: String,
    /// Provenance source.
    pub source: String,
    /// Task that produced the record, or the task it describes.
    pub task: String,
    /// Run outcome (`failed`, `completed`, …). Empty when there is no run.
    pub outcome: String,
    /// Linked node or source ids. Used for overlap and for the trace relationship.
    pub links: Vec<String>,
    /// True when a linked fingerprint no longer matches.
    pub stale: bool,
    /// True for a successful context-pack shape.
    pub pattern: bool,
    /// Intent kind the pattern was recorded for. Empty for other memories.
    pub intent_kind: String,
}

impl MemoryCandidate {
    /// Line mixed into the context cache key. Field order is fixed.
    pub fn cache_line(&self) -> String {
        format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            self.id,
            self.memory_type,
            self.status,
            self.stale,
            self.pattern,
            self.intent_kind,
            self.task,
            self.outcome,
            self.content,
            self.links.join(",")
        )
    }
}

/// A ranking decision for one candidate.
#[derive(Debug, Clone)]
pub(crate) struct MemoryDecision {
    pub id: String,
    pub fragment_id: String,
    pub memory_type: String,
    pub name: String,
    pub status: String,
    pub source: String,
    pub content: String,
    pub reason: String,
    pub relationship: String,
    pub score: ScoreParts,
    pub priority: u8,
    pub node_tokens: u32,
    pub fragment_tokens: u32,
    pub in_budget: bool,
}

/// Ranks memory for one task. Forgotten candidates are skipped, not traced.
pub(crate) fn rank(
    memory: &[MemoryCandidate],
    terms: &BTreeSet<String>,
    intent_kind: &str,
    routed: &BTreeSet<String>,
    matched: &BTreeSet<String>,
) -> Vec<MemoryDecision> {
    let mut ordered: Vec<&MemoryCandidate> = memory.iter().collect();
    ordered.sort_by(|left, right| left.id.cmp(&right.id));
    let mut seen = HashSet::new();
    let mut decisions = Vec::new();
    for candidate in ordered {
        if !seen.insert(candidate.id.clone()) || candidate.status == "forgotten" {
            continue;
        }
        decisions.push(decide(candidate, terms, intent_kind, routed, matched));
    }
    decisions
}

/// Budget rows for decisions that compete with graph items.
pub(crate) fn budget_items(decisions: &[MemoryDecision]) -> Vec<BudgetItem> {
    let mut items = Vec::new();
    for decision in decisions.iter().filter(|decision| decision.in_budget) {
        items.push(BudgetItem {
            key: node_key(&decision.id),
            priority: decision.priority,
            score: decision.score.total,
            tokens: decision.node_tokens,
        });
        items.push(BudgetItem {
            key: fragment_key(&decision.fragment_id),
            priority: decision.priority,
            score: decision.score.total,
            tokens: decision.fragment_tokens,
        });
    }
    items
}

/// Ids whose node or fragment survived the shared budget.
pub(crate) fn selected_ids(decisions: &[MemoryDecision], kept: &BTreeSet<String>) -> Vec<String> {
    let mut ids = Vec::new();
    for decision in decisions.iter().filter(|decision| decision.in_budget) {
        if kept.contains(&node_key(&decision.id))
            || kept.contains(&fragment_key(&decision.fragment_id))
        {
            ids.push(decision.id.clone());
        }
    }
    ids
}

/// Appends memory nodes, fragments, exclusions, and trace entries.
pub(crate) fn apply(
    decisions: &[MemoryDecision],
    kept: &BTreeSet<String>,
    selected_ids: &BTreeSet<String>,
    selected_nodes: &mut Vec<SelectedNode>,
    selected_fragments: &mut Vec<SelectedFragment>,
    exclusions: &mut Vec<Exclusion>,
    trace: &mut Vec<TraceEntry>,
) {
    for decision in decisions {
        let node_selected = selected_ids.contains(&decision.id);
        let fragment_selected = decision.in_budget
            && node_selected
            && kept.contains(&fragment_key(&decision.fragment_id));
        let node_reason = if decision.in_budget && !node_selected {
            "over_budget".to_owned()
        } else {
            decision.reason.clone()
        };
        let fragment_reason = if decision.in_budget && !fragment_selected {
            "over_budget".to_owned()
        } else {
            decision.reason.clone()
        };
        trace.push(entry(
            &decision.id,
            "node",
            node_selected,
            &node_reason,
            decision,
            decision.node_tokens,
        ));
        trace.push(entry(
            &decision.fragment_id,
            "fragment",
            fragment_selected,
            &fragment_reason,
            decision,
            decision.fragment_tokens,
        ));
        if node_selected {
            selected_nodes.push(SelectedNode {
                id: decision.id.clone(),
                node_type: decision.memory_type.clone(),
                name: decision.name.clone(),
                status: decision.status.clone(),
                priority: decision.priority,
                score: decision.score.total,
                source: decision.source.clone(),
                reason: node_reason,
                relationship: decision.relationship.clone(),
                token_estimate: decision.node_tokens,
            });
        } else {
            exclusions.push(Exclusion {
                id: decision.id.clone(),
                item_kind: "node".to_owned(),
                reason: node_reason,
                relationship: decision.relationship.clone(),
                source: decision.source.clone(),
                token_estimate: decision.node_tokens,
            });
        }
        if fragment_selected {
            selected_fragments.push(SelectedFragment {
                id: decision.fragment_id.clone(),
                node_id: decision.id.clone(),
                source_ref: decision.source.clone(),
                location: String::new(),
                kind: "memory".to_owned(),
                token_estimate: decision.fragment_tokens,
                content: decision.content.clone(),
                reason: fragment_reason,
            });
        } else {
            exclusions.push(Exclusion {
                id: decision.fragment_id.clone(),
                item_kind: "fragment".to_owned(),
                reason: fragment_reason,
                relationship: decision.relationship.clone(),
                source: decision.source.clone(),
                token_estimate: decision.fragment_tokens,
            });
        }
    }
}

fn entry(
    id: &str,
    item_kind: &str,
    selected: bool,
    reason: &str,
    decision: &MemoryDecision,
    token_estimate: u32,
) -> TraceEntry {
    TraceEntry {
        id: id.to_owned(),
        item_kind: item_kind.to_owned(),
        outcome: if selected {
            "selected".to_owned()
        } else {
            "excluded".to_owned()
        },
        reason: reason.to_owned(),
        relationship: decision.relationship.clone(),
        source: decision.source.clone(),
        token_estimate,
        priority: decision.priority,
        score: decision.score.clone(),
    }
}

fn decide(
    candidate: &MemoryCandidate,
    terms: &BTreeSet<String>,
    intent_kind: &str,
    routed: &BTreeSet<String>,
    matched: &BTreeSet<String>,
) -> MemoryDecision {
    let blob = format!(
        "{}\n{}\n{}\n{}\n{}\n{}",
        candidate.name,
        candidate.content,
        candidate.task,
        candidate.outcome,
        candidate.memory_type,
        candidate.links.join(" ")
    );
    let overlap = tokens(&blob);
    let shared: Vec<String> = overlap.intersection(terms).cloned().collect();
    let term = shared.first().cloned().unwrap_or_default();
    let linked = candidate
        .links
        .iter()
        .find(|id| matched.contains(id.as_str()) || routed.contains(id.as_str()));
    let context_hit = candidate.links.iter().any(|id| {
        routed.iter().any(|context| {
            tokens(id).intersection(&tokens(context)).next().is_some()
                || context.split('.').any(|part| tokens(id).contains(part))
        })
    });
    let mut score = ScoreParts {
        term: u32::from(!shared.is_empty()) * 1000,
        context: u32::from(context_hit) * 400,
        lexical: u32::try_from(shared.len())
            .unwrap_or(u32::MAX)
            .saturating_mul(50),
        graph: u32::from(linked.is_some()) * 200,
        semantic: 0,
        total: 0,
    };
    score.total = score
        .term
        .saturating_add(score.context)
        .saturating_add(score.lexical)
        .saturating_add(score.graph);
    let failure = is_failure(candidate);
    let intent_ok = candidate.pattern && candidate.intent_kind == intent_kind;
    let (reason, in_budget, status) = if candidate.pattern && candidate.stale && !shared.is_empty()
    {
        ("stale_pattern".to_owned(), false, "stale".to_owned())
    } else if candidate.pattern && !shared.is_empty() && !intent_ok {
        (
            "pattern_intent_mismatch".to_owned(),
            false,
            display_status(candidate),
        )
    } else if shared.is_empty() {
        (
            "not_addressed_by_task".to_owned(),
            false,
            display_status(candidate),
        )
    } else if candidate.pattern && intent_ok {
        (
            format!("context_pattern:{intent_kind}:{term}"),
            true,
            display_status(candidate),
        )
    } else if failure {
        let reason = format!("previous_failure:{term}");
        if candidate.stale {
            (format!("stale:{reason}"), true, "stale".to_owned())
        } else {
            (reason, true, display_status(candidate))
        }
    } else {
        let reason = format!("memory_term:{term}");
        if candidate.stale {
            (format!("stale:{reason}"), true, "stale".to_owned())
        } else {
            (reason, true, display_status(candidate))
        }
    };
    if candidate.stale && in_budget {
        score = down_rank(score);
    }
    let status_label = status.clone();
    MemoryDecision {
        id: candidate.id.clone(),
        fragment_id: format!("{}#0", candidate.id),
        memory_type: candidate.memory_type.clone(),
        name: candidate.name.clone(),
        status,
        source: candidate.source.clone(),
        content: candidate.content.clone(),
        reason,
        relationship: linked
            .map(|id| format!("linked:{id}"))
            .unwrap_or_else(|| format!("memory:{}", candidate.memory_type)),
        score,
        priority: 8,
        node_tokens: token_estimate(&format!(
            "{} {} {}",
            candidate.name, status_label, candidate.task
        )),
        fragment_tokens: token_estimate(&candidate.content),
        in_budget,
    }
}

fn is_failure(candidate: &MemoryCandidate) -> bool {
    matches!(candidate.outcome.as_str(), "failed" | "failure")
        && matches!(candidate.memory_type.as_str(), "lesson" | "episodic")
}

/// `canonical` is never shown. Stale wins over proposed and confirmed.
fn display_status(candidate: &MemoryCandidate) -> String {
    if candidate.stale {
        return "stale".to_owned();
    }
    match candidate.status.as_str() {
        "confirmed" => "confirmed".to_owned(),
        _ => "proposed".to_owned(),
    }
}

fn down_rank(mut score: ScoreParts) -> ScoreParts {
    score.term /= 4;
    score.context /= 4;
    score.lexical /= 4;
    score.graph /= 4;
    score.semantic /= 4;
    score.total = score
        .term
        .saturating_add(score.context)
        .saturating_add(score.lexical)
        .saturating_add(score.graph)
        .saturating_add(score.semantic);
    if score.total == 0 {
        score.term = 1;
        score.total = 1;
    }
    score
}

fn node_key(id: &str) -> String {
    format!("node:{id}")
}

fn fragment_key(id: &str) -> String {
    format!("fragment:{id}")
}

/// Cache digest of the candidate list. Empty input is not used; the caller keeps `context-v1`.
pub fn memory_fingerprint(memory: &[MemoryCandidate]) -> String {
    let mut lines: Vec<String> = memory.iter().map(MemoryCandidate::cache_line).collect();
    lines.sort();
    let mut hasher = sha2::Sha256::new();
    use sha2::Digest;
    for line in lines {
        hasher.update(line.as_bytes());
        hasher.update(b"\n");
    }
    let digest = hasher.finalize();
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

#[cfg(test)]
fn lesson(content: &str, stale: bool) -> MemoryCandidate {
    MemoryCandidate {
        id: "memory.lesson.example".to_owned(),
        memory_type: "lesson".to_owned(),
        name: "Idempotency".to_owned(),
        status: "proposed".to_owned(),
        content: content.to_owned(),
        source: "run:run-1".to_owned(),
        task: "Add retry support to payment webhook processing".to_owned(),
        outcome: "failed".to_owned(),
        links: vec!["app/payments/service.py".to_owned()],
        stale,
        pattern: false,
        intent_kind: String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn terms(task: &str) -> BTreeSet<String> {
        crate::tokens::task_tokens(task)
    }

    #[test]
    fn forgotten_is_omitted() {
        let mut item = lesson("payment webhook idempotency", false);
        item.status = "forgotten".to_owned();
        let ranked = rank(
            &[item],
            &terms("Add idempotency to payment webhook processing"),
            "feature",
            &BTreeSet::new(),
            &BTreeSet::new(),
        );
        assert!(ranked.is_empty());
    }

    #[test]
    fn canonical_status_is_shown_as_proposed() {
        let mut item = lesson("payment webhook idempotency", false);
        item.status = "canonical".to_owned();
        let ranked = rank(
            &[item],
            &terms("Add idempotency to payment webhook processing"),
            "feature",
            &BTreeSet::new(),
            &BTreeSet::new(),
        );
        assert_eq!(ranked[0].status, "proposed");
    }
}
