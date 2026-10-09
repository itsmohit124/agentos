//! Domain intelligence over the project knowledge graph.
//!
//! Heuristics write Observed, Inferred, or Proposed nodes. Confirmed and Canonical
//! exist only after [`confirm`]. Generated markdown omits audit timestamps.

mod heuristic;
mod lifecycle;
mod markdown;

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

pub use lifecycle::Lifecycle;

use crate::error::KnowledgeError;
use crate::model::{Finding, KnowledgeEdge, KnowledgeNode, NodeKind, TransitionRecord};
use crate::store::KnowledgeStore;
use crate::walk;

use lifecycle::{Origin, check, machine_status_forbidden, preserve_status};

/// Clock for transition audit rows. The value is not copied into golden markdown.
pub trait Clock: Send + Sync {
    /// A comparable instant. Tests use a fixed string.
    fn now(&self) -> String;
}

/// Wall clock, as Unix seconds.
#[derive(Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> String {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs().to_string())
            .unwrap_or_else(|_| "0".to_owned())
    }
}

/// Clock that always returns the same string.
#[derive(Debug, Clone)]
pub struct FixedClock {
    /// Value stored in `occurred_at`.
    pub instant: String,
}

impl Clock for FixedClock {
    fn now(&self) -> String {
        self.instant.clone()
    }
}

/// Future LLM-assisted proposals. No network implementation is shipped.
pub trait DomainProposer: Send + Sync {
    /// Short name recorded on transitions.
    fn name(&self) -> &str;
    /// Extra Proposed nodes. Confirmed and Canonical values are rejected.
    fn propose(&self, existing: &[KnowledgeNode]) -> Result<Vec<DomainProposal>, KnowledgeError>;
}

/// One proposer suggestion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DomainProposal {
    /// Node to insert. Status must be Proposed.
    pub node: KnowledgeNode,
    /// Edges whose endpoints already exist or are this node.
    pub edges: Vec<KnowledgeEdge>,
}

/// Proposer that adds nothing.
#[derive(Debug, Default)]
pub struct NullProposer;

impl DomainProposer for NullProposer {
    fn name(&self) -> &str {
        "none"
    }

    fn propose(&self, _existing: &[KnowledgeNode]) -> Result<Vec<DomainProposal>, KnowledgeError> {
        Ok(Vec::new())
    }
}

/// What [`scan`] wrote.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DomainReport {
    /// Domain nodes after the scan.
    pub nodes: usize,
    /// Domain edges after the scan.
    pub edges: usize,
    /// Findings after the scan.
    pub findings: usize,
    /// SQLite rows written. Zero when the domain graph is unchanged.
    pub rows_written: u64,
    /// Node counts by lifecycle status.
    pub by_status: BTreeMap<String, usize>,
}

/// What [`propose`] changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProposeReport {
    /// Nodes moved one step toward Proposed.
    pub promoted: usize,
    /// Proposer name.
    pub proposer: String,
    /// Nodes the proposer added.
    pub added: usize,
}

/// A confirm or reject result. The audit timestamp stays in the transition table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StatusChange {
    /// Domain node id.
    pub id: String,
    /// Status before the command.
    pub from: String,
    /// Status after the command.
    pub to: String,
    /// Who asked.
    pub actor: String,
    /// Why.
    pub reason: String,
}

/// Terminology list plus ambiguity findings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TerminologyView {
    /// Terminology nodes, sorted by id.
    pub terms: Vec<KnowledgeNode>,
    /// Ambiguity findings. These are not merges.
    pub ambiguities: Vec<Finding>,
}

/// Result of [`validate`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ValidationReport {
    /// True when drift, lifecycle, and provenance checks passed.
    pub ok: bool,
    /// Stored findings, including ambiguity and leaks.
    pub findings: Vec<Finding>,
    /// Confirmed or canonical nodes whose evidence files are gone.
    pub drift: Vec<String>,
    /// Confirmed or canonical nodes with no matching transition.
    pub lifecycle_violations: Vec<String>,
    /// Nodes or edges with incomplete provenance.
    pub provenance_incomplete: Vec<String>,
}

/// Scans the repository, then refreshes domain nodes from the software graph.
pub fn scan(root: &Path) -> Result<DomainReport, KnowledgeError> {
    scan_with(root, &SystemClock)
}

/// [`scan`] with an injected clock.
pub fn scan_with(root: &Path, clock: &dyn Clock) -> Result<DomainReport, KnowledgeError> {
    crate::scan::scan(root)?;
    let store = KnowledgeStore::open(root)?;
    let files = file_texts(root)?;
    let extracted = heuristic::extract(
        &store.software_nodes()?,
        &store.software_edges()?,
        &store.fragments()?,
        &files,
    );
    let existing = store.domain_nodes()?;
    let (nodes, transitions) = reconcile(&existing, extracted.nodes, clock);
    let software_ids: BTreeSet<String> = store
        .software_nodes()?
        .into_iter()
        .map(|node| node.id)
        .collect();
    let node_ids: BTreeSet<String> = nodes.iter().map(|node| node.id.clone()).collect();
    let edges = filter_edges(extracted.edges, &node_ids, &software_ids);
    let rows_written = store.sync_domain(&nodes, &edges, &extracted.findings, &transitions)?;
    markdown::write(root, &nodes, &edges)?;
    Ok(report(&nodes, &edges, &extracted.findings, rows_written))
}

/// Promotes inferred nodes one step and accepts Proposed nodes from `proposer`.
pub fn propose(root: &Path) -> Result<ProposeReport, KnowledgeError> {
    propose_with(root, &NullProposer, &SystemClock)
}

/// [`propose`] with an injected proposer and clock.
pub fn propose_with(
    root: &Path,
    proposer: &dyn DomainProposer,
    clock: &dyn Clock,
) -> Result<ProposeReport, KnowledgeError> {
    let store = KnowledgeStore::open(root)?;
    let mut nodes = store.domain_nodes()?;
    let mut edges = store.domain_edges()?;
    let findings = store.domain_findings()?;
    let mut transitions: Vec<TransitionRecord> = Vec::new();
    let mut promoted = 0usize;
    for node in &mut nodes {
        let Some(from) = Lifecycle::parse(&node.status) else {
            continue;
        };
        let Some(to) = from.forward() else {
            continue;
        };
        if machine_status_forbidden(to) {
            continue;
        }
        check(from, to)?;
        let index = store.transitions_for(&node.id)?.len()
            + transitions
                .iter()
                .filter(|item| item.node_id == node.id)
                .count();
        transitions.push(transition(
            &node.id,
            from,
            to,
            "agentos",
            "promote one lifecycle step",
            clock,
            index,
        ));
        node.status = to.as_str().to_owned();
        node.fingerprint = heuristic::fingerprint(node);
        promoted += 1;
    }
    let suggestions = proposer.propose(&nodes)?;
    let mut added = 0usize;
    for suggestion in suggestions {
        let status = Lifecycle::parse(&suggestion.node.status).ok_or_else(|| {
            KnowledgeError::ProposerForbidden {
                proposer: proposer.name().to_owned(),
                status: suggestion.node.status.clone(),
            }
        })?;
        if status != Origin::Proposer.initial() || machine_status_forbidden(status) {
            return Err(KnowledgeError::ProposerForbidden {
                proposer: proposer.name().to_owned(),
                status: status.as_str().to_owned(),
            });
        }
        if nodes.iter().any(|node| node.id == suggestion.node.id) {
            continue;
        }
        transitions.push(transition(
            &suggestion.node.id,
            Lifecycle::Proposed,
            Lifecycle::Proposed,
            proposer.name(),
            "proposal from proposer",
            clock,
            0,
        ));
        // Creation is recorded as none → proposed. Replace the self-pair above.
        if let Some(last) = transitions.last_mut() {
            last.from_status = "none".to_owned();
            last.id = transition_id(&suggestion.node.id, "none", Lifecycle::Proposed.as_str(), 0);
        }
        nodes.push(suggestion.node);
        edges.extend(suggestion.edges);
        added += 1;
    }
    nodes.sort_by(|a, b| a.id.cmp(&b.id));
    edges.sort_by(|a, b| a.id.cmp(&b.id));
    edges.dedup_by(|a, b| a.id == b.id);
    store.sync_domain(&nodes, &edges, &findings, &transitions)?;
    markdown::write(root, &nodes, &edges)?;
    Ok(ProposeReport {
        promoted,
        proposer: proposer.name().to_owned(),
        added,
    })
}

/// Moves Proposed → Confirmed, or Confirmed → Canonical when evidence still exists.
pub fn confirm(
    root: &Path,
    id: &str,
    actor: &str,
    reason: &str,
) -> Result<StatusChange, KnowledgeError> {
    confirm_with(root, id, actor, reason, &SystemClock)
}

/// [`confirm`] with an injected clock.
pub fn confirm_with(
    root: &Path,
    id: &str,
    actor: &str,
    reason: &str,
    clock: &dyn Clock,
) -> Result<StatusChange, KnowledgeError> {
    require_text(id, "actor", actor)?;
    require_text(id, "reason", reason)?;
    let store = KnowledgeStore::open(root)?;
    let mut nodes = store.domain_nodes()?;
    let node = nodes
        .iter_mut()
        .find(|node| node.id == id)
        .ok_or_else(|| KnowledgeError::NotFound(id.to_owned()))?;
    let from = Lifecycle::parse(&node.status).ok_or_else(|| KnowledgeError::IllegalTransition {
        from: node.status.clone(),
        to: Lifecycle::Confirmed.as_str().to_owned(),
    })?;
    let to = match from {
        Lifecycle::Proposed => Lifecycle::Confirmed,
        Lifecycle::Confirmed => Lifecycle::Canonical,
        other => {
            return Err(KnowledgeError::IllegalTransition {
                from: other.as_str().to_owned(),
                to: Lifecycle::Confirmed.as_str().to_owned(),
            });
        }
    };
    check(from, to)?;
    if to == Lifecycle::Canonical {
        if !node.provenance.is_complete() || node.provenance.source.is_empty() {
            return Err(KnowledgeError::DomainUpdate {
                id: id.to_owned(),
                reason: "provenance is incomplete".to_owned(),
            });
        }
        for path in &node.source_refs {
            if !root.join(path).is_file() {
                return Err(KnowledgeError::DomainUpdate {
                    id: id.to_owned(),
                    reason: format!("missing evidence {path}"),
                });
            }
        }
    }
    let index = store.transitions_for(id)?.len();
    let change = StatusChange {
        id: id.to_owned(),
        from: from.as_str().to_owned(),
        to: to.as_str().to_owned(),
        actor: actor.to_owned(),
        reason: reason.to_owned(),
    };
    node.status = to.as_str().to_owned();
    node.fingerprint = heuristic::fingerprint(node);
    let edges = store.domain_edges()?;
    let findings = store.domain_findings()?;
    let record = transition(id, from, to, actor, reason, clock, index);
    store.sync_domain(&nodes, &edges, &findings, &[record])?;
    markdown::write(root, &nodes, &edges)?;
    Ok(change)
}

/// Moves a non-canonical node to Rejected.
pub fn reject(
    root: &Path,
    id: &str,
    actor: &str,
    reason: &str,
) -> Result<StatusChange, KnowledgeError> {
    reject_with(root, id, actor, reason, &SystemClock)
}

/// [`reject`] with an injected clock.
pub fn reject_with(
    root: &Path,
    id: &str,
    actor: &str,
    reason: &str,
    clock: &dyn Clock,
) -> Result<StatusChange, KnowledgeError> {
    require_text(id, "actor", actor)?;
    require_text(id, "reason", reason)?;
    let store = KnowledgeStore::open(root)?;
    let mut nodes = store.domain_nodes()?;
    let node = nodes
        .iter_mut()
        .find(|node| node.id == id)
        .ok_or_else(|| KnowledgeError::NotFound(id.to_owned()))?;
    let from = Lifecycle::parse(&node.status).ok_or_else(|| KnowledgeError::IllegalTransition {
        from: node.status.clone(),
        to: Lifecycle::Rejected.as_str().to_owned(),
    })?;
    if !from.can_reject() {
        return Err(KnowledgeError::IllegalTransition {
            from: from.as_str().to_owned(),
            to: Lifecycle::Rejected.as_str().to_owned(),
        });
    }
    check(from, Lifecycle::Rejected)?;
    let index = store.transitions_for(id)?.len();
    node.status = Lifecycle::Rejected.as_str().to_owned();
    node.fingerprint = heuristic::fingerprint(node);
    let edges = store.domain_edges()?;
    let findings = store.domain_findings()?;
    store.sync_domain(
        &nodes,
        &edges,
        &findings,
        &[transition(
            id,
            from,
            Lifecycle::Rejected,
            actor,
            reason,
            clock,
            index,
        )],
    )?;
    markdown::write(root, &nodes, &edges)?;
    Ok(StatusChange {
        id: id.to_owned(),
        from: from.as_str().to_owned(),
        to: Lifecycle::Rejected.as_str().to_owned(),
        actor: actor.to_owned(),
        reason: reason.to_owned(),
    })
}

/// Bounded contexts, sorted by id.
pub fn contexts(root: &Path) -> Result<Vec<KnowledgeNode>, KnowledgeError> {
    Ok(KnowledgeStore::open(root)?
        .domain_nodes()?
        .into_iter()
        .filter(|node| node.node_type == NodeKind::BoundedContext)
        .collect())
}

/// Terminology nodes and ambiguity findings.
pub fn terminology(root: &Path) -> Result<TerminologyView, KnowledgeError> {
    let store = KnowledgeStore::open(root)?;
    let terms = store
        .domain_nodes()?
        .into_iter()
        .filter(|node| node.node_type == NodeKind::Terminology)
        .collect();
    let ambiguities = store
        .domain_findings()?
        .into_iter()
        .filter(|finding| finding.kind == "terminology_ambiguity")
        .collect();
    Ok(TerminologyView { terms, ambiguities })
}

/// Context-map edges, sorted by id.
pub fn context_map(root: &Path) -> Result<Vec<KnowledgeEdge>, KnowledgeError> {
    Ok(KnowledgeStore::open(root)?
        .domain_edges()?
        .into_iter()
        .filter(|edge| edge.relation == crate::model::RelationType::CrossContextTo)
        .collect())
}

/// Checks drift, lifecycle evidence, and provenance. Does not change statuses.
pub fn validate(root: &Path) -> Result<ValidationReport, KnowledgeError> {
    let store = KnowledgeStore::open(root)?;
    let nodes = store.domain_nodes()?;
    let edges = store.domain_edges()?;
    let findings = store.domain_findings()?;
    let mut drift = Vec::new();
    let mut lifecycle_violations = Vec::new();
    let mut provenance_incomplete = Vec::new();
    for node in &nodes {
        if !provenance_ok_node(node) {
            provenance_incomplete.push(node.id.clone());
        }
        let Some(status) = Lifecycle::parse(&node.status) else {
            lifecycle_violations.push(format!("{} has unknown status {}", node.id, node.status));
            continue;
        };
        if !matches!(status, Lifecycle::Confirmed | Lifecycle::Canonical) {
            continue;
        }
        let transitions = store.transitions_for(&node.id)?;
        if !transitions
            .iter()
            .any(|record| record.to_status == status.as_str())
        {
            lifecycle_violations.push(format!(
                "{} is {} without a transition",
                node.id,
                status.as_str()
            ));
        }
        for path in &node.source_refs {
            if !root.join(path).is_file() {
                drift.push(format!("{} missing evidence {path}", node.id));
            }
        }
    }
    for edge in &edges {
        if !edge.provenance.is_complete() || edge.provenance.source.is_empty() {
            provenance_incomplete.push(edge.id.clone());
        }
    }
    provenance_incomplete.sort();
    provenance_incomplete.dedup();
    Ok(ValidationReport {
        ok: drift.is_empty() && lifecycle_violations.is_empty() && provenance_incomplete.is_empty(),
        findings,
        drift,
        lifecycle_violations,
        provenance_incomplete,
    })
}

fn reconcile(
    existing: &[KnowledgeNode],
    desired: Vec<KnowledgeNode>,
    clock: &dyn Clock,
) -> (Vec<KnowledgeNode>, Vec<TransitionRecord>) {
    let existing_by_id: BTreeMap<&str, &KnowledgeNode> = existing
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect();
    let mut seen = BTreeSet::new();
    let mut nodes = Vec::new();
    let mut transitions = Vec::new();
    for mut node in desired {
        seen.insert(node.id.clone());
        if let Some(old) = existing_by_id.get(node.id.as_str()) {
            if let (Some(stored), Some(fresh)) = (
                Lifecycle::parse(&old.status),
                Lifecycle::parse(&node.status),
            ) && preserve_status(stored, fresh)
            {
                node.status = old.status.clone();
                node.fingerprint = heuristic::fingerprint(&node);
            }
            nodes.push(node);
        } else {
            let status = Lifecycle::parse(&node.status).unwrap_or(Lifecycle::Inferred);
            transitions.push(transition(
                &node.id,
                status,
                status,
                "agentos",
                creation_reason(status),
                clock,
                0,
            ));
            if let Some(last) = transitions.last_mut() {
                last.from_status = "none".to_owned();
                last.id = transition_id(&node.id, "none", status.as_str(), 0);
            }
            nodes.push(node);
        }
    }
    for old in existing {
        if seen.contains(&old.id) {
            continue;
        }
        if Lifecycle::parse(&old.status).is_some_and(|status| {
            matches!(
                status,
                Lifecycle::Confirmed | Lifecycle::Canonical | Lifecycle::Rejected
            )
        }) {
            nodes.push(old.clone());
        }
    }
    nodes.sort_by(|a, b| a.id.cmp(&b.id));
    (nodes, transitions)
}

fn creation_reason(status: Lifecycle) -> &'static str {
    match status {
        Lifecycle::Observed => "declared glossary term",
        Lifecycle::Proposed => "greenfield requirement",
        _ => "deterministic domain heuristic",
    }
}

fn transition(
    node_id: &str,
    from: Lifecycle,
    to: Lifecycle,
    actor: &str,
    reason: &str,
    clock: &dyn Clock,
    index: usize,
) -> TransitionRecord {
    TransitionRecord {
        id: transition_id(node_id, from.as_str(), to.as_str(), index),
        node_id: node_id.to_owned(),
        from_status: from.as_str().to_owned(),
        to_status: to.as_str().to_owned(),
        actor: actor.to_owned(),
        reason: reason.to_owned(),
        occurred_at: clock.now(),
    }
}

fn transition_id(node_id: &str, from: &str, to: &str, index: usize) -> String {
    format!("{node_id}|{from}|{to}|{index}")
}

fn filter_edges(
    mut edges: Vec<KnowledgeEdge>,
    nodes: &BTreeSet<String>,
    software: &BTreeSet<String>,
) -> Vec<KnowledgeEdge> {
    edges.retain(|edge| {
        nodes.contains(&edge.from_id)
            && (nodes.contains(&edge.to_id) || software.contains(&edge.to_id))
    });
    edges.sort_by(|a, b| a.id.cmp(&b.id));
    edges.dedup_by(|a, b| a.id == b.id);
    edges
}

fn report(
    nodes: &[KnowledgeNode],
    edges: &[KnowledgeEdge],
    findings: &[Finding],
    rows_written: u64,
) -> DomainReport {
    let mut by_status = BTreeMap::new();
    for node in nodes {
        *by_status.entry(node.status.clone()).or_default() += 1;
    }
    DomainReport {
        nodes: nodes.len(),
        edges: edges.len(),
        findings: findings.len(),
        rows_written,
        by_status,
    }
}

fn file_texts(root: &Path) -> Result<BTreeMap<String, String>, KnowledgeError> {
    let mut files = BTreeMap::new();
    for file in walk::list_sources(root)? {
        let Ok(text) = String::from_utf8(file.bytes) else {
            continue;
        };
        files.insert(file.relative, text);
    }
    Ok(files)
}

fn require_text(id: &str, field: &str, value: &str) -> Result<(), KnowledgeError> {
    if value.trim().is_empty() {
        Err(KnowledgeError::DomainUpdate {
            id: id.to_owned(),
            reason: format!("{field} must not be empty"),
        })
    } else {
        Ok(())
    }
}

fn provenance_ok_node(node: &KnowledgeNode) -> bool {
    node.provenance.is_complete()
        && !node.provenance.source.is_empty()
        && !node.provenance.location.is_empty()
        && node.confidence_millis > 0
}

#[cfg(test)]
mod tests;
