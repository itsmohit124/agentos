//! Retrieval pipeline.
//!
//! intent → bounded-context routing → graph membership → lexical/FTS → semantic seam →
//! ranking → token budget → Context Pack and retrieval trace.
//!
//! Routing uses ubiquitous-language names. A call or cross-context edge into a context the
//! task did not name is recorded and not followed. That is what keeps a payment task out of
//! Notifications. Nothing here calls a model or the network.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use agentos_knowledge::{
    Finding, KnowledgeEdge, KnowledgeFragment, KnowledgeNode, NodeKind, RelationType,
    token_estimate,
};

use crate::budget::{self, BudgetItem};
use crate::cache::{self, cache_key};
use crate::error::ContextError;
use crate::pack::{
    Budget, ConstraintLink, ContextPack, ContextStats, Exclusion, Intent, RefItem, RetrievalTrace,
    ScoreParts, SelectedFragment, SelectedNode, TermHit, TerminologyEntry, TraceEntry,
};
use crate::semantic::SemanticRetriever;
use crate::tokens::{self, is_mandatory, task_tokens, tokens};
use crate::view::KnowledgeView;

/// Budget and cache location for one build.
#[derive(Debug, Clone, Default)]
pub struct ContextConfig {
    /// Token ceiling. `None` keeps every relevant item and still does not dump the repository.
    pub budget: Option<u32>,
    /// Directory of cached packs. `None` disables the cache.
    pub cache_dir: Option<PathBuf>,
}

/// Builds a Context Pack. A cache hit is byte-identical to the JSON of a fresh build.
pub fn build(
    view: &impl KnowledgeView,
    task: &str,
    config: &ContextConfig,
    semantic: &dyn SemanticRetriever,
) -> Result<ContextPack, ContextError> {
    Ok(serde_json::from_str(&render_json(
        view, task, config, semantic,
    )?)?)
}

/// Canonical JSON, including a trailing newline.
pub fn render_json(
    view: &impl KnowledgeView,
    task: &str,
    config: &ContextConfig,
    semantic: &dyn SemanticRetriever,
) -> Result<String, ContextError> {
    if let Some(dir) = &config.cache_dir {
        let fingerprint = view.fingerprint()?;
        let key = cache_key(&fingerprint, task, config.budget);
        if let Some(hit) = cache::read(dir, &key)?
            && serde_json::from_str::<ContextPack>(&hit).is_ok()
        {
            tracing::debug!(key, "context cache hit");
            return Ok(hit);
        }
        let pack = assemble(view, task, config.budget, semantic)?;
        let json = to_json(&pack)?;
        cache::write(dir, &key, &json)?;
        return Ok(json);
    }
    to_json(&assemble(view, task, config.budget, semantic)?)
}

/// Stats for a pack.
pub fn stats(pack: &ContextPack) -> ContextStats {
    ContextStats::from_pack(pack)
}

/// Pretty JSON plus a newline.
pub fn to_json(pack: &ContextPack) -> Result<String, ContextError> {
    let mut text = serde_json::to_string_pretty(pack)?;
    text.push('\n');
    Ok(text)
}

fn assemble(
    view: &impl KnowledgeView,
    task: &str,
    limit: Option<u32>,
    semantic: &dyn SemanticRetriever,
) -> Result<ContextPack, ContextError> {
    let nodes = view.nodes()?;
    let edges = view.edges()?;
    let fragments = view.fragments()?;
    let findings = view.findings()?;
    let fts_ids = view.fts(task)?;
    let (kind, rule) = tokens::intent_kind(task);
    let mandatory = is_mandatory(task);
    let terms = task_tokens(task);
    let semantic_hits = semantic.search(task, &nodes);

    let contexts: Vec<&KnowledgeNode> = nodes
        .iter()
        .filter(|node| node.node_type == NodeKind::BoundedContext && node.status != "rejected")
        .collect();
    let route = route_contexts(&nodes, &contexts, &terms);
    let mut lexical_outside = BTreeSet::new();
    let mut semantic_outside = BTreeSet::new();
    let mut semantic_score: BTreeMap<String, u32> = BTreeMap::new();

    for hit in &semantic_hits {
        if in_routed(&hit.node_id, &contexts, &route.contexts) {
            semantic_score.insert(hit.node_id.clone(), hit.score);
        } else {
            semantic_outside.insert(hit.node_id.clone());
        }
    }
    for fragment_id in &fts_ids {
        if let Some(fragment) = fragments.iter().find(|item| &item.id == fragment_id)
            && !in_routed(&fragment.node_id, &contexts, &route.contexts)
        {
            lexical_outside.insert(fragment.id.clone());
        }
    }
    for fragment in &fragments {
        if fragment_overlap(fragment, &terms) == 0 {
            continue;
        }
        if !in_routed(&fragment.node_id, &contexts, &route.contexts)
            && package_of(&fragment.node_id).is_some()
        {
            lexical_outside.insert(fragment.id.clone());
        }
    }

    let mut candidates: BTreeSet<String> = BTreeSet::new();
    let mut reasons: BTreeMap<String, String> = BTreeMap::new();
    for node in &nodes {
        if node.status == "rejected" {
            continue;
        }
        if in_routed(&node.id, &contexts, &route.contexts) {
            candidates.insert(node.id.clone());
            reasons.insert(node.id.clone(), node_reason(node, &route, &terms));
        } else if is_document(node)
            && let Some(reason) = document_reason(node, &edges, &fragments, &contexts, &route)
        {
            candidates.insert(node.id.clone());
            reasons.insert(node.id.clone(), reason);
        }
    }

    let anchors = {
        let mut anchors = candidates.clone();
        anchors.extend(route.contexts.iter().cloned());
        anchors.extend(route.matched.iter().cloned());
        anchors
    };

    let mut items = Vec::new();
    let mut label_tokens: BTreeMap<String, u32> = BTreeMap::new();
    let mut node_priority: BTreeMap<String, u8> = BTreeMap::new();
    let mut node_score: BTreeMap<String, ScoreParts> = BTreeMap::new();
    for node in &nodes {
        if !candidates.contains(&node.id) {
            continue;
        }
        let parts = score_of(node, &fragments, &edges, &route, &terms, &semantic_score);
        let priority = node_priority_of(node, mandatory);
        let tokens = label_cost(node);
        node_score.insert(node.id.clone(), parts.clone());
        node_priority.insert(node.id.clone(), priority);
        label_tokens.insert(node.id.clone(), tokens);
        items.push(BudgetItem {
            key: node_key(&node.id),
            priority,
            score: parts.total,
            tokens,
        });
        for fragment in fragments.iter().filter(|item| item.node_id == node.id) {
            if fragment_covered_by_child(&node.id, &edges, &fragments) {
                continue;
            }
            items.push(BudgetItem {
                key: fragment_key(&fragment.id),
                priority: fragment_priority_of(node, priority),
                score: parts.total,
                tokens: fragment.token_estimate,
            });
        }
    }

    let kept = budget::allocate(&items, limit);
    let mut selected_ids = BTreeSet::new();
    for node in &nodes {
        if !candidates.contains(&node.id) {
            continue;
        }
        let fragment_kept = fragments.iter().any(|fragment| {
            fragment.node_id == node.id && kept.contains(&fragment_key(&fragment.id))
        });
        if kept.contains(&node_key(&node.id)) || fragment_kept {
            selected_ids.insert(node.id.clone());
        }
    }

    let mut used = 0u32;
    let mut dropped = 0u32;
    for item in &items {
        if kept.contains(&item.key) {
            let node_dropped = item.key.starts_with("node:")
                && !selected_ids.contains(item.key.trim_start_matches("node:"));
            if node_dropped {
                dropped += 1;
                continue;
            }
            used = used.saturating_add(item.tokens);
        } else {
            dropped += 1;
        }
    }

    let mut selected_nodes = Vec::new();
    let mut exclusions = Vec::new();
    let mut trace = Vec::new();
    for node in &nodes {
        let parts = node_score
            .get(&node.id)
            .cloned()
            .unwrap_or_else(|| score_of(node, &fragments, &edges, &route, &terms, &semantic_score));
        let priority = node_priority
            .get(&node.id)
            .copied()
            .unwrap_or_else(|| node_priority_of(node, mandatory));
        let relationship = relationship(&node.id, &edges, &anchors);
        let source = node.provenance.source.clone();
        let tokens = label_tokens
            .get(&node.id)
            .copied()
            .unwrap_or_else(|| label_cost(node));
        let selected = selected_ids.contains(&node.id);
        let reason = if selected {
            reasons
                .get(&node.id)
                .cloned()
                .unwrap_or_else(|| "routed_context".to_owned())
        } else if candidates.contains(&node.id) {
            "over_budget".to_owned()
        } else {
            exclusion_reason(
                node,
                &contexts,
                &semantic_outside,
                &lexical_outside,
                &fragments,
            )
        };
        trace.push(TraceEntry {
            id: node.id.clone(),
            item_kind: "node".to_owned(),
            outcome: if selected {
                "selected".to_owned()
            } else {
                "excluded".to_owned()
            },
            reason: reason.clone(),
            relationship: relationship.clone(),
            source: source.clone(),
            token_estimate: tokens,
            priority,
            score: parts.clone(),
        });
        if selected {
            selected_nodes.push(SelectedNode {
                id: node.id.clone(),
                node_type: node.node_type.as_str().to_owned(),
                name: node.name.clone(),
                status: node.status.clone(),
                priority,
                score: parts.total,
                source,
                reason,
                relationship,
                token_estimate: tokens,
            });
        } else {
            exclusions.push(Exclusion {
                id: node.id.clone(),
                item_kind: "node".to_owned(),
                reason,
                relationship,
                source,
                token_estimate: tokens,
            });
        }
    }

    let mut selected_fragments = Vec::new();
    for fragment in &fragments {
        let node = nodes.iter().find(|node| node.id == fragment.node_id);
        let Some(node) = node else {
            continue;
        };
        let traced = candidates.contains(&node.id) || lexical_outside.contains(&fragment.id);
        if !traced {
            continue;
        }
        let covered = fragment_covered_by_child(&node.id, &edges, &fragments);
        let selected = !covered
            && selected_ids.contains(&node.id)
            && kept.contains(&fragment_key(&fragment.id));
        let reason = if selected {
            reasons
                .get(&node.id)
                .cloned()
                .unwrap_or_else(|| "routed_context".to_owned())
        } else if covered && selected_ids.contains(&node.id) {
            "covered_by_child_fragment".to_owned()
        } else if candidates.contains(&node.id) {
            "over_budget".to_owned()
        } else {
            "lexical_outside_routed_context".to_owned()
        };
        let parts = node_score
            .get(&node.id)
            .cloned()
            .unwrap_or_else(ScoreParts::default);
        let priority = fragment_priority_of(node, node_priority_of(node, mandatory));
        let relationship = relationship(&node.id, &edges, &anchors);
        trace.push(TraceEntry {
            id: fragment.id.clone(),
            item_kind: "fragment".to_owned(),
            outcome: if selected {
                "selected".to_owned()
            } else {
                "excluded".to_owned()
            },
            reason: reason.clone(),
            relationship: relationship.clone(),
            source: fragment.source_ref.clone(),
            token_estimate: fragment.token_estimate,
            priority,
            score: parts,
        });
        if selected {
            selected_fragments.push(SelectedFragment {
                id: fragment.id.clone(),
                node_id: fragment.node_id.clone(),
                source_ref: fragment.source_ref.clone(),
                location: fragment.location.clone(),
                kind: fragment.kind.clone(),
                token_estimate: fragment.token_estimate,
                content: fragment.content.clone(),
                reason,
            });
        } else {
            exclusions.push(Exclusion {
                id: fragment.id.clone(),
                item_kind: "fragment".to_owned(),
                reason,
                relationship,
                source: fragment.source_ref.clone(),
                token_estimate: fragment.token_estimate,
            });
        }
    }

    selected_nodes.sort_by(|left, right| left.id.cmp(&right.id));
    selected_fragments.sort_by(|left, right| left.id.cmp(&right.id));
    exclusions.sort_by(|left, right| {
        left.id
            .cmp(&right.id)
            .then(left.item_kind.cmp(&right.item_kind))
    });
    trace.sort_by(|left, right| {
        left.id
            .cmp(&right.id)
            .then(left.item_kind.cmp(&right.item_kind))
    });

    let pack = ContextPack {
        task: task.to_owned(),
        intent: Intent {
            kind,
            rule,
            contexts: route.contexts.into_iter().collect(),
            terms: route.terms,
        },
        budget: Budget {
            limit,
            used,
            dropped,
        },
        terminology: terminology(&selected_ids, &nodes, &edges, &findings),
        architecture: refs_where(&selected_ids, &nodes, |node| {
            node.node_type == NodeKind::Module || (is_document(node) && !is_readme(node))
        }),
        constraints: constraints(&selected_ids, &edges),
        invariants: refs_where(&selected_ids, &nodes, |node| {
            node.node_type == NodeKind::Invariant
        }),
        decisions: refs_where(&selected_ids, &nodes, |node| {
            node.node_type == NodeKind::Decision
        }),
        tests: refs_where(&selected_ids, &nodes, |node| {
            node.node_type == NodeKind::Test
        }),
        selected_nodes,
        selected_fragments,
        exclusions,
        retrieval_trace: RetrievalTrace { entries: trace },
    };
    Ok(pack)
}

struct Route {
    contexts: BTreeSet<String>,
    terms: Vec<TermHit>,
    matched: BTreeSet<String>,
    routed_terms: BTreeSet<String>,
}

fn route_contexts(
    nodes: &[KnowledgeNode],
    contexts: &[&KnowledgeNode],
    terms: &BTreeSet<String>,
) -> Route {
    let mut route = Route {
        contexts: BTreeSet::new(),
        terms: Vec::new(),
        matched: BTreeSet::new(),
        routed_terms: BTreeSet::new(),
    };
    for node in nodes {
        if node.status == "rejected" {
            continue;
        }
        let overlap: BTreeSet<String> = match_tokens(node).intersection(terms).cloned().collect();
        if overlap.is_empty() {
            continue;
        }
        let Some(context_id) = route_target(node, &overlap, contexts) else {
            continue;
        };
        route.contexts.insert(context_id.clone());
        route.matched.insert(node.id.clone());
        for term in overlap {
            route.routed_terms.insert(term.clone());
            route.terms.push(TermHit {
                task_term: term,
                node_id: node.id.clone(),
                context_id: context_id.clone(),
            });
        }
    }
    route.terms.sort_by(|left, right| {
        left.task_term
            .cmp(&right.task_term)
            .then(left.node_id.cmp(&right.node_id))
    });
    route.terms.dedup();
    route
}

fn route_target(
    node: &KnowledgeNode,
    overlap: &BTreeSet<String>,
    contexts: &[&KnowledgeNode],
) -> Option<String> {
    let context_id = context_of(node, contexts)?;
    if node.node_type.is_domain() {
        return Some(context_id);
    }
    let own_name = tokens(&node.name);
    let useful = overlap
        .iter()
        .any(|term| own_name.contains(term) && !names_other_context(term, contexts, &context_id));
    useful.then_some(context_id)
}

/// `notification` on a users test names the Notifications context, so it must not route Users.
fn names_other_context(term: &str, contexts: &[&KnowledgeNode], own_context: &str) -> bool {
    contexts
        .iter()
        .any(|context| context.id != own_context && tokens(&context.name).contains(term))
}

fn context_of(node: &KnowledgeNode, contexts: &[&KnowledgeNode]) -> Option<String> {
    if node.node_type == NodeKind::BoundedContext {
        return Some(node.id.clone());
    }
    let package = package_of(&node.id)?;
    context_for_package(package, contexts).map(str::to_owned)
}

fn context_for_package<'a>(package: &str, contexts: &[&'a KnowledgeNode]) -> Option<&'a str> {
    let package_tokens = tokens(package);
    let mut fuzzy = None;
    for context in contexts {
        if context.name.eq_ignore_ascii_case(package) {
            return Some(context.id.as_str());
        }
        if fuzzy.is_none()
            && tokens(&context.name)
                .intersection(&package_tokens)
                .next()
                .is_some()
        {
            fuzzy = Some(context.id.as_str());
        }
    }
    fuzzy
}

fn in_routed(id: &str, contexts: &[&KnowledgeNode], routed: &BTreeSet<String>) -> bool {
    package_of(id)
        .and_then(|package| context_for_package(package, contexts))
        .is_some_and(|context_id| routed.contains(context_id))
}

fn package_of(id: &str) -> Option<&str> {
    if let Some(rest) = id.strip_prefix("domain.") {
        return rest.split('.').next();
    }
    if let Some(rest) = id.strip_prefix("software.app.") {
        return rest.split('.').next();
    }
    if let Some(rest) = id.strip_prefix("quality.tests.test_") {
        return rest.split('.').next();
    }
    None
}

fn match_tokens(node: &KnowledgeNode) -> BTreeSet<String> {
    let mut set = tokens(&node.name);
    if node.node_type.is_domain() || is_document(node) {
        set.extend(tokens(&node.summary));
    }
    set
}

fn node_reason(node: &KnowledgeNode, route: &Route, terms: &BTreeSet<String>) -> String {
    if let Some(term) = match_tokens(node).intersection(terms).next() {
        format!("task_term:{term}")
    } else if let Some(context) = route.contexts.iter().find(|context| {
        context_of(node, &[]).is_some() || package_matches_context(&node.id, context)
    }) {
        let _ = context;
        format!(
            "routed_context:{}",
            route
                .contexts
                .iter()
                .find(|context| package_matches_context(&node.id, context))
                .map(String::as_str)
                .unwrap_or("routed")
        )
    } else {
        "routed_context".to_owned()
    }
}

fn package_matches_context(id: &str, context_id: &str) -> bool {
    let Some(package) = package_of(id) else {
        return false;
    };
    context_id
        .split('.')
        .any(|part| part.eq_ignore_ascii_case(package) || tokens(part).contains(package))
        || tokens(context_id).contains(package)
        || tokens(context_id)
            .intersection(&tokens(package))
            .next()
            .is_some()
}

fn document_reason(
    node: &KnowledgeNode,
    edges: &[KnowledgeEdge],
    fragments: &[KnowledgeFragment],
    contexts: &[&KnowledgeNode],
    route: &Route,
) -> Option<String> {
    if let Some(edge) = edges.iter().find(|edge| {
        edge.relation == RelationType::References
            && edge.from_id == node.id
            && in_routed(&edge.to_id, contexts, &route.contexts)
    }) {
        return Some(format!("references:{}", edge.to_id));
    }
    let mut named = tokens(&node.name);
    named.extend(tokens(&node.summary));
    if let Some(term) = named.intersection(&route.routed_terms).next() {
        return Some(format!("task_term:{term}"));
    }
    for fragment in fragments.iter().filter(|item| item.node_id == node.id) {
        if let Some(term) = tokens(&fragment.content)
            .intersection(&route.routed_terms)
            .next()
        {
            return Some(format!("lexical:{term}"));
        }
    }
    None
}

fn exclusion_reason(
    node: &KnowledgeNode,
    contexts: &[&KnowledgeNode],
    semantic_outside: &BTreeSet<String>,
    lexical_outside: &BTreeSet<String>,
    fragments: &[KnowledgeFragment],
) -> String {
    if node.status == "rejected" {
        return "rejected".to_owned();
    }
    if semantic_outside.contains(&node.id) {
        return "semantic_outside_routed_context".to_owned();
    }
    if fragments
        .iter()
        .any(|fragment| fragment.node_id == node.id && lexical_outside.contains(&fragment.id))
    {
        return "lexical_outside_routed_context".to_owned();
    }
    if package_of(&node.id).is_some_and(|package| context_for_package(package, contexts).is_some())
    {
        return "outside_routed_context".to_owned();
    }
    "not_addressed_by_task".to_owned()
}

fn score_of(
    node: &KnowledgeNode,
    fragments: &[KnowledgeFragment],
    edges: &[KnowledgeEdge],
    route: &Route,
    terms: &BTreeSet<String>,
    semantic_score: &BTreeMap<String, u32>,
) -> ScoreParts {
    let term = u32::from(match_tokens(node).intersection(terms).next().is_some()) * 1000;
    let context = u32::from(
        route
            .contexts
            .iter()
            .any(|context| package_matches_context(&node.id, context) || node.id == *context),
    ) * 400;
    let mut lexical_terms = match_tokens(node);
    for fragment in fragments.iter().filter(|item| item.node_id == node.id) {
        lexical_terms.extend(tokens(&fragment.content));
    }
    let lexical = u32::try_from(lexical_terms.intersection(terms).count())
        .unwrap_or(u32::MAX)
        .saturating_mul(50);
    let graph = u32::from(edges.iter().any(|edge| {
        let (from, to) = (&edge.from_id, &edge.to_id);
        let touches = from == &node.id || to == &node.id;
        let other = if from == &node.id { to } else { from };
        touches && (route.matched.contains(other) || route.contexts.contains(other))
    })) * 200;
    let semantic = semantic_score.get(&node.id).copied().unwrap_or(0);
    ScoreParts {
        term,
        context,
        lexical,
        graph,
        semantic,
        total: term
            .saturating_add(context)
            .saturating_add(lexical)
            .saturating_add(graph)
            .saturating_add(semantic),
    }
}

fn node_priority_of(node: &KnowledgeNode, mandatory: bool) -> u8 {
    if is_readme(node) {
        return 8;
    }
    match node.node_type {
        NodeKind::Invariant if mandatory => 1,
        NodeKind::BoundedContext
        | NodeKind::Entity
        | NodeKind::ValueObject
        | NodeKind::Aggregate
        | NodeKind::Terminology
        | NodeKind::DomainService
        | NodeKind::Repository
        | NodeKind::Policy
        | NodeKind::DomainEvent
        | NodeKind::AntiCorruptionLayer => 2,
        NodeKind::Module => 3,
        NodeKind::Document => 3,
        NodeKind::Decision => 4,
        NodeKind::Invariant | NodeKind::Test => 5,
        NodeKind::Class
        | NodeKind::Function
        | NodeKind::Method
        | NodeKind::Interface
        | NodeKind::Symbol => 6,
        NodeKind::File | NodeKind::Import => 7,
    }
}

fn fragment_priority_of(node: &KnowledgeNode, node_priority: u8) -> u8 {
    if is_readme(node) {
        return 8;
    }
    match node.node_type {
        NodeKind::Decision => 4,
        NodeKind::Document => 3,
        NodeKind::Invariant | NodeKind::Test => 5,
        NodeKind::File | NodeKind::Import => 7,
        NodeKind::Class
        | NodeKind::Function
        | NodeKind::Method
        | NodeKind::Interface
        | NodeKind::Symbol => 6,
        _ => node_priority,
    }
}

fn is_document(node: &KnowledgeNode) -> bool {
    matches!(node.node_type, NodeKind::Document | NodeKind::Decision)
}

fn is_readme(node: &KnowledgeNode) -> bool {
    node.id == "document.README"
        || node.name.eq_ignore_ascii_case("readme")
        || node.source_id.ends_with("README.md")
}

fn label_cost(node: &KnowledgeNode) -> u32 {
    token_estimate(&format!("{} {} {}", node.name, node.status, node.summary))
}

fn fragment_overlap(fragment: &KnowledgeFragment, terms: &BTreeSet<String>) -> usize {
    tokens(&fragment.content).intersection(terms).count()
}

fn node_key(id: &str) -> String {
    format!("node:{id}")
}

fn fragment_key(id: &str) -> String {
    format!("fragment:{id}")
}

/// A class body repeats its methods. Keep the method fragments and omit the parent copy.
fn fragment_covered_by_child(
    node_id: &str,
    edges: &[KnowledgeEdge],
    fragments: &[KnowledgeFragment],
) -> bool {
    edges.iter().any(|edge| {
        edge.relation == RelationType::Contains
            && edge.from_id == node_id
            && fragments
                .iter()
                .any(|fragment| fragment.node_id == edge.to_id && fragment.token_estimate > 0)
    })
}

fn relationship(node_id: &str, edges: &[KnowledgeEdge], anchors: &BTreeSet<String>) -> String {
    let mut best: Option<&KnowledgeEdge> = None;
    for edge in edges {
        let other = if edge.from_id == node_id {
            edge.to_id.as_str()
        } else if edge.to_id == node_id {
            edge.from_id.as_str()
        } else {
            continue;
        };
        if !anchors.contains(other) {
            continue;
        }
        best = Some(match best {
            None => edge,
            Some(current) if prefer_edge(edge, current) => edge,
            Some(current) => current,
        });
    }
    best.map(|edge| {
        format!(
            "{} {} -> {}",
            edge.relation.as_str(),
            edge.from_id,
            edge.to_id
        )
    })
    .unwrap_or_default()
}

fn prefer_edge(candidate: &KnowledgeEdge, current: &KnowledgeEdge) -> bool {
    let rank = relation_rank(candidate.relation).cmp(&relation_rank(current.relation));
    rank.is_lt() || (rank.is_eq() && candidate.id < current.id)
}

fn relation_rank(relation: RelationType) -> u8 {
    match relation {
        RelationType::DerivedFrom => 0,
        RelationType::BelongsTo => 1,
        RelationType::HasEntity => 2,
        RelationType::HasValueObject => 3,
        RelationType::HasAggregate => 4,
        RelationType::ConstrainedBy => 5,
        RelationType::CrossContextTo => 6,
        RelationType::References => 7,
        RelationType::Tests => 8,
        RelationType::Calls => 9,
        RelationType::Contains => 10,
        RelationType::Imports => 11,
        RelationType::Governs => 12,
        RelationType::AntiCorrupts => 13,
        RelationType::ConflictsWith => 14,
        RelationType::Extends | RelationType::Implements => 15,
    }
}

fn terminology(
    selected: &BTreeSet<String>,
    nodes: &[KnowledgeNode],
    edges: &[KnowledgeEdge],
    findings: &[Finding],
) -> Vec<TerminologyEntry> {
    let mut entries = Vec::new();
    for node in nodes {
        if node.node_type != NodeKind::Terminology || !selected.contains(&node.id) {
            continue;
        }
        let mut conflicts = Vec::new();
        for edge in edges {
            if edge.relation != RelationType::ConflictsWith {
                continue;
            }
            if edge.from_id == node.id {
                conflicts.push(edge.to_id.clone());
            } else if edge.to_id == node.id {
                conflicts.push(edge.from_id.clone());
            }
        }
        conflicts.sort();
        conflicts.dedup();
        let mut finding_ids: Vec<String> = findings
            .iter()
            .filter(|finding| finding.subjects.iter().any(|subject| subject == &node.id))
            .map(|finding| finding.id.clone())
            .collect();
        finding_ids.sort();
        entries.push(TerminologyEntry {
            id: node.id.clone(),
            name: node.name.clone(),
            status: node.status.clone(),
            source: node.provenance.source.clone(),
            conflicts,
            finding_ids,
        });
    }
    entries.sort_by(|left, right| left.id.cmp(&right.id));
    entries
}

fn refs_where(
    selected: &BTreeSet<String>,
    nodes: &[KnowledgeNode],
    pred: impl Fn(&KnowledgeNode) -> bool,
) -> Vec<RefItem> {
    let mut items = Vec::new();
    for node in nodes {
        if selected.contains(&node.id) && pred(node) {
            items.push(RefItem {
                id: node.id.clone(),
                name: node.name.clone(),
                status: node.status.clone(),
                source: node.provenance.source.clone(),
                summary: node.summary.clone(),
            });
        }
    }
    items.sort_by(|left, right| left.id.cmp(&right.id));
    items
}

fn constraints(selected: &BTreeSet<String>, edges: &[KnowledgeEdge]) -> Vec<ConstraintLink> {
    let mut links = Vec::new();
    for edge in edges {
        if !matches!(
            edge.relation,
            RelationType::Imports | RelationType::Calls | RelationType::ConstrainedBy
        ) {
            continue;
        }
        if selected.contains(&edge.from_id) && selected.contains(&edge.to_id) {
            links.push(ConstraintLink {
                from_id: edge.from_id.clone(),
                relation: edge.relation.as_str().to_owned(),
                to_id: edge.to_id.clone(),
                source: edge.provenance.source.clone(),
            });
        }
    }
    links.sort_by(|left, right| {
        left.from_id
            .cmp(&right.from_id)
            .then(left.relation.cmp(&right.relation))
            .then(left.to_id.cmp(&right.to_id))
    });
    links
}
