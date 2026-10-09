//! Deterministic markdown for confirmed, canonical, and proposed domain knowledge.
//!
//! Observed and inferred nodes are omitted. Timestamps are omitted.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::error::KnowledgeError;
use crate::model::{KnowledgeEdge, KnowledgeNode, NodeKind, RelationType};

use super::lifecycle::Lifecycle;

/// Renders the five domain documents. Keys are file names.
pub(crate) fn render(nodes: &[KnowledgeNode], edges: &[KnowledgeEdge]) -> BTreeMap<String, String> {
    let included: Vec<&KnowledgeNode> = nodes
        .iter()
        .filter(|node| {
            Lifecycle::parse(&node.status).is_some_and(|status| {
                matches!(
                    status,
                    Lifecycle::Proposed | Lifecycle::Confirmed | Lifecycle::Canonical
                )
            })
        })
        .collect();
    let mut out = BTreeMap::new();
    out.insert(
        "bounded-contexts.md".to_owned(),
        document(
            "Bounded contexts",
            &section(&included, NodeKind::BoundedContext),
        ),
    );
    out.insert(
        "context-map.md".to_owned(),
        document("Context map", &map_section(&included, edges)),
    );
    out.insert(
        "ubiquitous-language.md".to_owned(),
        document(
            "Ubiquitous language",
            &section(&included, NodeKind::Terminology),
        ),
    );
    out.insert(
        "aggregates.md".to_owned(),
        document("Aggregates", &section(&included, NodeKind::Aggregate)),
    );
    out.insert(
        "invariants.md".to_owned(),
        document("Invariants", &section(&included, NodeKind::Invariant)),
    );
    out
}

/// Writes [`render`] under `.agentos/knowledge/domain/`. Identical bytes are left untouched.
pub(crate) fn write(
    root: &Path,
    nodes: &[KnowledgeNode],
    edges: &[KnowledgeEdge],
) -> Result<(), KnowledgeError> {
    let dir = root.join(".agentos/knowledge/domain");
    fs::create_dir_all(&dir)?;
    for (name, body) in render(nodes, edges) {
        let path = dir.join(name);
        let current = fs::read_to_string(&path).unwrap_or_default();
        if current != body {
            fs::write(path, body)?;
        }
    }
    Ok(())
}

fn document(title: &str, body: &str) -> String {
    format!("# {title}\n\n{body}")
}

fn section(nodes: &[&KnowledgeNode], kind: NodeKind) -> String {
    let mut confirmed = String::new();
    let mut proposed = String::new();
    for node in nodes.iter().filter(|node| node.node_type == kind) {
        let line = bullet(node);
        match Lifecycle::parse(&node.status) {
            Some(Lifecycle::Confirmed | Lifecycle::Canonical) => confirmed.push_str(&line),
            Some(Lifecycle::Proposed) => proposed.push_str(&line),
            _ => {}
        }
    }
    format!(
        "## Confirmed and canonical\n\n{}\n## Proposed\n\n{}",
        filled(&confirmed),
        filled(&proposed)
    )
}

fn map_section(nodes: &[&KnowledgeNode], edges: &[KnowledgeEdge]) -> String {
    let ids: BTreeMap<&str, &KnowledgeNode> =
        nodes.iter().map(|node| (node.id.as_str(), *node)).collect();
    let mut confirmed = String::new();
    let mut proposed = String::new();
    let mut rows: Vec<&KnowledgeEdge> = edges
        .iter()
        .filter(|edge| edge.relation == RelationType::CrossContextTo)
        .filter(|edge| {
            ids.contains_key(edge.from_id.as_str()) && ids.contains_key(edge.to_id.as_str())
        })
        .collect();
    rows.sort_by(|a, b| a.id.cmp(&b.id));
    for edge in rows {
        let from = ids[edge.from_id.as_str()];
        let to = ids[edge.to_id.as_str()];
        let downstream = edge
            .metadata
            .get("downstream")
            .map(String::as_str)
            .unwrap_or("?");
        let upstream = edge
            .metadata
            .get("upstream")
            .map(String::as_str)
            .unwrap_or("?");
        let line = format!(
            "- `{from}` → `{to}` (downstream {downstream}, upstream {upstream})\n",
            from = from.id,
            to = to.id
        );
        let rank = |status: &str| {
            Lifecycle::parse(status)
                .and_then(Lifecycle::rank)
                .unwrap_or(0)
        };
        if rank(&from.status) >= 3 && rank(&to.status) >= 3 {
            confirmed.push_str(&line);
        } else {
            proposed.push_str(&line);
        }
    }
    format!(
        "## Confirmed and canonical\n\n{}\n## Proposed\n\n{}",
        filled(&confirmed),
        filled(&proposed)
    )
}

fn bullet(node: &KnowledgeNode) -> String {
    let evidence = node
        .payload
        .get("evidence")
        .map(String::as_str)
        .unwrap_or("");
    format!(
        "- **{name}** (`{id}`) — {summary} Evidence: `{source}` via {method}, confidence {confidence}/1000. {evidence}\n",
        name = node.name,
        id = node.id,
        summary = node.summary,
        source = node.provenance.source,
        method = node.provenance.method,
        confidence = node.confidence_millis,
    )
}

fn filled(body: &str) -> &str {
    if body.is_empty() { "(none)\n" } else { body }
}
