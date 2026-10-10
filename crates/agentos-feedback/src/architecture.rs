//! Deterministic architecture detectors over the existing knowledge graph.
//!
//! The detectors do not build a second graph. They read nodes, edges, and fragments produced by
//! `scan` and `domain_scan`. A missing edge is not treated as a violation.
//!
//! Shallow-module heuristic: a non-test file is shallow when it contains exactly one class,
//! function, or interface, and that symbol is a function or interface, or a class with at most
//! one method. Package markers with zero symbols are not shallow. The threshold is structural,
//! not a tuned quality score.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use agentos_knowledge::{
    KnowledgeEdge, KnowledgeFragment, KnowledgeNode, NodeKind, RelationType, domain_scan,
};
use serde::Serialize;

use crate::config::ArchitectureConfig;
use crate::error::FeedbackError;
use crate::result::ResultProvenance;

/// Infrastructure import roots. The same list the domain heuristic uses for hexagonal leaks.
const INFRA: &[&str] = &[
    "sqlalchemy",
    "django",
    "boto3",
    "botocore",
    "redis",
    "pymongo",
    "psycopg",
    "psycopg2",
    "requests",
    "httpx",
    "celery",
    "kafka",
    "pika",
    "asyncpg",
    "sqlite3",
    "pymysql",
];

/// Domain concepts that should have a test when they were derived from a software symbol.
const TESTED: [NodeKind; 4] = [
    NodeKind::Entity,
    NodeKind::ValueObject,
    NodeKind::Aggregate,
    NodeKind::DomainService,
];

/// One architecture finding. Subjects are sorted. Edge findings list the importer, then the
/// imported file. Other path lists are sorted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ArchitectureFinding {
    /// `{detector}:{paths}`.
    pub id: String,
    /// Detector name.
    pub detector: String,
    /// One sentence.
    pub summary: String,
    /// Source files that explain the finding. An import edge lists the importer first, then the
    /// imported file. Other lists are sorted and unique.
    pub paths: Vec<String>,
    /// Knowledge node ids, sorted.
    pub subjects: Vec<String>,
    /// The edge, import, or count that produced the finding.
    pub evidence: String,
    /// Heuristic provenance. These findings are inferred, not canonical.
    pub provenance: ResultProvenance,
}

/// Detector output. Findings are sorted by detector, then id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ArchitectureReport {
    /// Findings. Empty when nothing was detected.
    pub findings: Vec<ArchitectureFinding>,
}

/// The graph slice the detectors read.
pub struct GraphSnapshot<'a> {
    /// Software and domain nodes.
    pub nodes: &'a [KnowledgeNode],
    /// Software and domain edges.
    pub edges: &'a [KnowledgeEdge],
    /// Fragments, used to read ADR text.
    pub fragments: &'a [KnowledgeFragment],
}

/// Scans `root` with the existing knowledge engine and runs the detectors.
pub fn analyze_project(
    root: &Path,
    config: &ArchitectureConfig,
) -> Result<ArchitectureReport, FeedbackError> {
    domain_scan(root)?;
    let store = agentos_knowledge::KnowledgeStore::open(root)?;
    let nodes = store.nodes()?;
    let edges = store.edges()?;
    let fragments = store.fragments()?;
    Ok(analyze(
        &GraphSnapshot {
            nodes: &nodes,
            edges: &edges,
            fragments: &fragments,
        },
        config,
    ))
}

/// Runs every detector. Output order does not depend on input order.
pub fn analyze(graph: &GraphSnapshot<'_>, config: &ArchitectureConfig) -> ArchitectureReport {
    let mut findings = Vec::new();
    findings.extend(circular_dependencies(graph));
    findings.extend(fat_controllers(graph, config));
    findings.extend(domain_infrastructure(graph));
    findings.extend(duplicate_terminology(graph));
    findings.extend(missing_tests(graph));
    findings.extend(shallow_modules(graph));
    findings.extend(boundary_leakage(graph));
    findings.extend(unwanted_direction(graph));
    findings.extend(oversized_surfaces(graph, config));
    findings.sort_by(|left, right| (&left.detector, &left.id).cmp(&(&right.detector, &right.id)));
    findings.dedup_by(|left, right| left.id == right.id);
    ArchitectureReport { findings }
}

fn circular_dependencies(graph: &GraphSnapshot<'_>) -> Vec<ArchitectureFinding> {
    let modules = modules_of(graph.nodes);
    let mut adj: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for edge in graph.edges {
        if edge.relation != RelationType::Imports {
            continue;
        }
        if modules.contains_key(edge.from_id.as_str()) && modules.contains_key(edge.to_id.as_str())
        {
            adj.entry(edge.from_id.as_str())
                .or_default()
                .insert(edge.to_id.as_str());
        }
    }
    find_cycles(&adj)
        .into_iter()
        .map(|cycle| {
            let paths = unique_paths(
                cycle
                    .iter()
                    .filter_map(|id| modules.get(id.as_str()).map(|n| n.source_id.as_str())),
            );
            let summary = format!(
                "import cycle {}",
                cycle
                    .iter()
                    .map(|id| modules
                        .get(id.as_str())
                        .map(|n| n.name.as_str())
                        .unwrap_or(id))
                    .collect::<Vec<_>>()
                    .join(" -> ")
            );
            finding(
                "circular_dependency",
                &summary,
                paths,
                cycle,
                &summary,
                "import-cycle",
                true,
            )
        })
        .collect()
}

fn fat_controllers(
    graph: &GraphSnapshot<'_>,
    config: &ArchitectureConfig,
) -> Vec<ArchitectureFinding> {
    graph
        .nodes
        .iter()
        .filter(|node| node.node_type == NodeKind::Class && node.name.ends_with("Controller"))
        .filter(|node| node.id.starts_with("software."))
        .filter_map(|node| {
            let methods = method_count(graph.nodes, node);
            if methods <= config.fat_controller_methods {
                return None;
            }
            let summary = format!(
                "{} has {methods} methods; the fat-controller threshold is {}",
                node.name, config.fat_controller_methods
            );
            Some(finding(
                "fat_controller",
                &summary,
                vec![node.source_id.clone()],
                vec![node.id.clone()],
                &format!("method count {methods}"),
                "method-count",
                true,
            ))
        })
        .collect()
}

fn domain_infrastructure(graph: &GraphSnapshot<'_>) -> Vec<ArchitectureFinding> {
    let mut out = Vec::new();
    for node in graph.nodes {
        if node.node_type != NodeKind::Import || !is_domain_core(&node.source_id) {
            continue;
        }
        let Some(target) = node.payload.get("target").or(node.payload.get("import")) else {
            continue;
        };
        let root = target.split(['.', '/']).next().unwrap_or(target);
        if !INFRA.contains(&root) {
            continue;
        }
        if adr_allows(graph, root) {
            continue;
        }
        let summary = format!("{root} is imported from domain file {}", node.source_id);
        out.push(finding(
            "domain_infrastructure",
            &summary,
            vec![node.source_id.clone()],
            vec![node.id.clone()],
            &format!("import {target}"),
            "import-root",
            true,
        ));
    }
    out
}

fn duplicate_terminology(graph: &GraphSnapshot<'_>) -> Vec<ArchitectureFinding> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for edge in graph.edges {
        if edge.relation != RelationType::ConflictsWith {
            continue;
        }
        let pair = sorted_pair(&edge.from_id, &edge.to_id);
        if !seen.insert(pair.clone()) {
            continue;
        }
        let left = node_by_id(graph.nodes, &edge.from_id);
        let right = node_by_id(graph.nodes, &edge.to_id);
        if left.is_none_or(|node| node.node_type != NodeKind::Terminology)
            || right.is_none_or(|node| node.node_type != NodeKind::Terminology)
        {
            continue;
        }
        let mut paths = Vec::new();
        if !edge.source_id.is_empty() {
            paths.push(edge.source_id.clone());
        }
        if let Some(node) = left {
            paths.push(node.source_id.clone());
        }
        if let Some(node) = right {
            paths.push(node.source_id.clone());
        }
        let names = format!(
            "{} / {}",
            left.map(|n| n.name.as_str())
                .unwrap_or(edge.from_id.as_str()),
            right
                .map(|n| n.name.as_str())
                .unwrap_or(edge.to_id.as_str())
        );
        let summary = format!("{names} are duplicate terms and were not merged");
        out.push(finding(
            "duplicate_terminology",
            &summary,
            paths,
            vec![pair.0, pair.1],
            "CONFLICTS_WITH edge between terminology nodes",
            "conflicts-with",
            true,
        ));
    }
    out
}

fn missing_tests(graph: &GraphSnapshot<'_>) -> Vec<ArchitectureFinding> {
    graph
        .nodes
        .iter()
        .filter(|node| TESTED.contains(&node.node_type))
        .filter_map(|node| {
            let software_id = node
                .payload
                .get("software_id")
                .cloned()
                .filter(|id| !id.is_empty())
                .or_else(|| derived_from(graph.edges, &node.id))?;
            if is_tested(graph, &software_id) {
                return None;
            }
            let path = node_by_id(graph.nodes, &software_id)
                .map(|software| software.source_id.clone())
                .filter(|path| !path.is_empty())
                .unwrap_or_else(|| node.source_id.clone());
            let summary = format!("{} has no test edge", node.name);
            Some(finding(
                "missing_tests",
                &summary,
                vec![path],
                vec![node.id.clone(), software_id],
                "no TESTS edge to the software symbol or its methods",
                "missing-tests-edge",
                true,
            ))
        })
        .collect()
}

fn shallow_modules(graph: &GraphSnapshot<'_>) -> Vec<ArchitectureFinding> {
    symbols_by_file(graph.nodes)
        .into_iter()
        .filter_map(|(path, symbols)| {
            if symbols.len() != 1 {
                return None;
            }
            let symbol = &symbols[0];
            let thin = match symbol.node_type {
                NodeKind::Class => method_count(graph.nodes, symbol) <= 1,
                NodeKind::Function | NodeKind::Interface => true,
                _ => false,
            };
            if !thin {
                return None;
            }
            let summary = format!(
                "{} is a shallow module: one {} with no further surface",
                path,
                symbol.node_type.as_str()
            );
            Some(finding(
                "shallow_module",
                &summary,
                vec![path],
                vec![symbol.id.clone()],
                "exactly one class, function, or interface, and at most one method",
                "shallow-module",
                true,
            ))
        })
        .collect()
}

fn boundary_leakage(graph: &GraphSnapshot<'_>) -> Vec<ArchitectureFinding> {
    directed_imports(graph, |from, to| {
        rank_of(from) == Some(Rank::Domain) && rank_of(to) == Some(Rank::Outer)
    })
    .into_iter()
    .map(|(from, to, from_node, to_node)| {
        let summary = format!(
            "domain module {} imports outer module {}",
            from_node.name, to_node.name
        );
        finding(
            "boundary_leakage",
            &summary,
            vec![from, to],
            vec![from_node.id, to_node.id],
            "IMPORTS from a domain module to an adapter, infrastructure, or controller module",
            "import-edge",
            false,
        )
    })
    .collect()
}

fn unwanted_direction(graph: &GraphSnapshot<'_>) -> Vec<ArchitectureFinding> {
    directed_imports(graph, |from, to| {
        let (Some(from_rank), Some(to_rank)) = (rank_of(from), rank_of(to)) else {
            return false;
        };
        if from_rank == Rank::Domain && to_rank == Rank::Outer {
            return false;
        }
        (from_rank as u8) < (to_rank as u8)
    })
    .into_iter()
    .map(|(from, to, from_node, to_node)| {
        let summary = format!("{} depends outward on {}", from_node.name, to_node.name);
        finding(
            "unwanted_direction",
            &summary,
            vec![from, to],
            vec![from_node.id, to_node.id],
            "IMPORTS goes from an inner layer to an outer layer",
            "import-edge",
            false,
        )
    })
    .collect()
}

fn oversized_surfaces(
    graph: &GraphSnapshot<'_>,
    config: &ArchitectureConfig,
) -> Vec<ArchitectureFinding> {
    symbols_by_file(graph.nodes)
        .into_iter()
        .filter_map(|(path, symbols)| {
            if symbols.len() <= config.public_surface_max {
                return None;
            }
            let summary = format!(
                "{path} exposes {} public symbols; the threshold is {}",
                symbols.len(),
                config.public_surface_max
            );
            let subjects = symbols.iter().map(|node| node.id.clone()).collect();
            Some(finding(
                "oversized_surface",
                &summary,
                vec![path],
                subjects,
                &format!("class/function/interface count {}", symbols.len()),
                "public-count",
                true,
            ))
        })
        .collect()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Rank {
    Domain = 0,
    Port = 1,
    Application = 2,
    Outer = 3,
}

fn rank_of(path: &str) -> Option<Rank> {
    if segment(
        path,
        &[
            "adapter",
            "adapters",
            "infra",
            "infrastructure",
            "controller",
            "controllers",
        ],
    ) {
        return Some(Rank::Outer);
    }
    if segment(path, &["application"]) {
        return Some(Rank::Application);
    }
    if segment(path, &["port", "ports"]) {
        return Some(Rank::Port);
    }
    if segment(path, &["domain"]) || stem_is(path, &["model", "models", "entity", "entities"]) {
        return Some(Rank::Domain);
    }
    None
}

fn segment(path: &str, names: &[&str]) -> bool {
    path.split(['/', '\\']).any(|part| {
        let stem = part.rsplit_once('.').map(|(stem, _)| stem).unwrap_or(part);
        names.contains(&part) || names.contains(&stem)
    })
}

fn stem_is(path: &str, names: &[&str]) -> bool {
    let file = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let stem = file.rsplit_once('.').map(|(stem, _)| stem).unwrap_or(file);
    names.contains(&stem)
}

fn is_domain_core(path: &str) -> bool {
    if path.contains("/adapter") || path.contains("/infra") || path.contains("/controller") {
        return false;
    }
    path.contains("/domain/") || stem_is(path, &["model", "models", "entity", "entities"])
}

fn adr_allows(graph: &GraphSnapshot<'_>, library: &str) -> bool {
    let library = library.to_ascii_lowercase();
    let decisions: BTreeSet<&str> = graph
        .nodes
        .iter()
        .filter(|node| node.node_type == NodeKind::Decision)
        .map(|node| node.id.as_str())
        .collect();
    graph.fragments.iter().any(|fragment| {
        if !decisions.contains(fragment.node_id.as_str()) {
            return false;
        }
        let lower = fragment.content.to_ascii_lowercase();
        lower.contains(&library)
            && (lower.contains("exception")
                || lower.contains("allowed")
                || lower.contains("waived"))
    })
}

fn modules_of(nodes: &[KnowledgeNode]) -> BTreeMap<&str, &KnowledgeNode> {
    nodes
        .iter()
        .filter(|node| node.node_type == NodeKind::Module && node.id.starts_with("software."))
        .map(|node| (node.id.as_str(), node))
        .collect()
}

fn node_by_id<'a>(nodes: &'a [KnowledgeNode], id: &str) -> Option<&'a KnowledgeNode> {
    nodes.iter().find(|node| node.id == id)
}

fn method_count(nodes: &[KnowledgeNode], class: &KnowledgeNode) -> usize {
    nodes
        .iter()
        .filter(|node| {
            node.node_type == NodeKind::Method
                && node.parent.as_deref() == Some(class.name.as_str())
                && node.scope == class.scope
                && node.source_id == class.source_id
        })
        .count()
}

fn symbols_by_file(nodes: &[KnowledgeNode]) -> BTreeMap<String, Vec<&KnowledgeNode>> {
    let mut grouped: BTreeMap<String, Vec<&KnowledgeNode>> = BTreeMap::new();
    for node in nodes {
        if !node.id.starts_with("software.") {
            continue;
        }
        if !matches!(
            node.node_type,
            NodeKind::Class | NodeKind::Function | NodeKind::Interface
        ) {
            continue;
        }
        if node.source_id.is_empty() || node.source_id.ends_with("__init__.py") {
            continue;
        }
        grouped
            .entry(node.source_id.clone())
            .or_default()
            .push(node);
    }
    for symbols in grouped.values_mut() {
        symbols.sort_by(|left, right| left.id.cmp(&right.id));
    }
    grouped
}

fn derived_from(edges: &[KnowledgeEdge], from: &str) -> Option<String> {
    edges.iter().find_map(|edge| {
        (edge.from_id == from && edge.relation == RelationType::DerivedFrom)
            .then(|| edge.to_id.clone())
    })
}

fn is_tested(graph: &GraphSnapshot<'_>, software_id: &str) -> bool {
    let methods: BTreeSet<&str> = graph
        .nodes
        .iter()
        .find(|node| node.id == software_id)
        .map(|class| {
            graph
                .nodes
                .iter()
                .filter(|node| {
                    node.node_type == NodeKind::Method
                        && node.parent.as_deref() == Some(class.name.as_str())
                        && node.scope == class.scope
                })
                .map(|node| node.id.as_str())
                .collect()
        })
        .unwrap_or_default();
    graph.edges.iter().any(|edge| {
        edge.relation == RelationType::Tests
            && (edge.to_id == software_id || methods.contains(edge.to_id.as_str()))
    })
}

fn directed_imports(
    graph: &GraphSnapshot<'_>,
    pred: impl Fn(&str, &str) -> bool,
) -> Vec<(String, String, KnowledgeNode, KnowledgeNode)> {
    let modules = modules_of(graph.nodes);
    let mut out = Vec::new();
    for edge in graph.edges {
        if edge.relation != RelationType::Imports {
            continue;
        }
        let (Some(from), Some(to)) = (
            modules.get(edge.from_id.as_str()),
            modules.get(edge.to_id.as_str()),
        ) else {
            continue;
        };
        if from.source_id.is_empty() || to.source_id.is_empty() {
            continue;
        }
        if !pred(&from.source_id, &to.source_id) {
            continue;
        }
        out.push((
            from.source_id.clone(),
            to.source_id.clone(),
            (*from).clone(),
            (*to).clone(),
        ));
    }
    out.sort_by(|left, right| (&left.0, &left.1).cmp(&(&right.0, &right.1)));
    out.dedup_by(|left, right| left.0 == right.0 && left.1 == right.1);
    out
}

fn find_cycles(adj: &BTreeMap<&str, BTreeSet<&str>>) -> Vec<Vec<String>> {
    let mut ids: Vec<&str> = adj.keys().copied().collect();
    for targets in adj.values() {
        ids.extend(targets.iter().copied());
    }
    ids.sort_unstable();
    ids.dedup();
    let mut color: BTreeMap<&str, u8> = BTreeMap::new();
    let mut stack = Vec::new();
    let mut found = Vec::new();
    for id in ids {
        if color.get(id).copied().unwrap_or(0) == 0 {
            visit(id, adj, &mut color, &mut stack, &mut found);
        }
    }
    found.sort();
    found.dedup();
    found
}

fn visit<'a>(
    id: &'a str,
    adj: &BTreeMap<&'a str, BTreeSet<&'a str>>,
    color: &mut BTreeMap<&'a str, u8>,
    stack: &mut Vec<&'a str>,
    found: &mut Vec<Vec<String>>,
) {
    color.insert(id, 1);
    stack.push(id);
    if let Some(nexts) = adj.get(id) {
        for next in nexts {
            match color.get(next).copied().unwrap_or(0) {
                1 => {
                    if let Some(start) = stack.iter().position(|item| item == next) {
                        let cycle = stack[start..]
                            .iter()
                            .map(|item| (*item).to_owned())
                            .collect();
                        found.push(canonical(cycle));
                    }
                }
                0 => visit(next, adj, color, stack, found),
                _ => {}
            }
        }
    }
    stack.pop();
    color.insert(id, 2);
}

fn canonical(mut cycle: Vec<String>) -> Vec<String> {
    if cycle.is_empty() {
        return cycle;
    }
    let min = cycle.iter().min().cloned().unwrap_or_default();
    if let Some(pos) = cycle.iter().position(|id| id == &min) {
        cycle.rotate_left(pos);
    }
    let mut reversed = cycle.clone();
    reversed.reverse();
    if let Some(pos) = reversed.iter().position(|id| id == &min) {
        reversed.rotate_left(pos);
    }
    if reversed < cycle { reversed } else { cycle }
}

fn unique_paths(paths: impl Iterator<Item = impl Into<String>>) -> Vec<String> {
    let mut out: Vec<String> = paths
        .map(Into::into)
        .filter(|path| !path.is_empty())
        .collect();
    out.sort();
    out.dedup();
    out
}

fn sorted_pair(left: &str, right: &str) -> (String, String) {
    if left <= right {
        (left.to_owned(), right.to_owned())
    } else {
        (right.to_owned(), left.to_owned())
    }
}

fn finding(
    detector: &str,
    summary: &str,
    paths: Vec<String>,
    subjects: Vec<String>,
    evidence: &str,
    method: &str,
    sort_paths: bool,
) -> ArchitectureFinding {
    let mut paths = paths;
    if sort_paths {
        paths.sort();
    }
    paths.dedup();
    let mut subjects = subjects;
    subjects.sort();
    subjects.dedup();
    let id = if paths.is_empty() {
        format!("{detector}:{}", subjects.join("+"))
    } else {
        format!("{detector}:{}", paths.join("+"))
    };
    ArchitectureFinding {
        id,
        detector: detector.to_owned(),
        summary: summary.to_owned(),
        paths: paths.clone(),
        subjects,
        evidence: evidence.to_owned(),
        provenance: ResultProvenance {
            kind: "inferred".to_owned(),
            source: paths.first().cloned().unwrap_or_default(),
            location: String::new(),
            extractor: "architecture".to_owned(),
            extractor_version: "1".to_owned(),
            method: method.to_owned(),
        },
    }
}
