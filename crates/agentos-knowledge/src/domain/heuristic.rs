//! Deterministic domain heuristics.
//!
//! Classifications are inferred or proposed. This module never emits Confirmed or Canonical.

use std::collections::{BTreeMap, BTreeSet};

use crate::id::is_adr;
use crate::model::{
    CONFIDENCE_INFERRED, Finding, KnowledgeEdge, KnowledgeFragment, KnowledgeNode, NODE_VERSION,
    NodeKind, Provenance, ProvenanceKind, RelationType, edge_id, line_span,
};

use super::lifecycle::{Lifecycle, Origin};

const ROLE: &[&str] = &[
    "models",
    "model",
    "entities",
    "entity",
    "service",
    "services",
    "repository",
    "repositories",
    "controller",
    "controllers",
    "schema",
    "domain",
    "ports",
    "port",
    "adapters",
    "adapter",
    "infra",
    "infrastructure",
    "dto",
    "dtos",
    "policy",
    "policies",
    "event",
    "events",
];

const PACKAGE_ROOTS: &[&str] = &["app", "src", "lib"];
const GENERIC: &[&str] = &["main", "common", "shared", "core", "util", "utils"];
const VALUE_NAMES: &[&str] = &["Money", "Address", "Email", "Currency", "PhoneNumber"];
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
const SYNONYMS: &[&[&str]] = &[
    &["payment", "charge", "transaction"],
    &["user", "customer", "account"],
    &["notification", "message", "alert"],
    &["order", "purchase"],
    &["invoice", "bill"],
];

/// Nodes, edges, and findings produced from one graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Extracted {
    /// Domain nodes, sorted by id.
    pub nodes: Vec<KnowledgeNode>,
    /// Domain edges, sorted by id.
    pub edges: Vec<KnowledgeEdge>,
    /// Findings, sorted by id.
    pub findings: Vec<Finding>,
}

struct ClassInfo {
    id: String,
    name: String,
    scope: String,
    source: String,
    summary: String,
    body: String,
    location: String,
    context: String,
}

struct Built {
    node: KnowledgeNode,
}

/// Reads the software graph. Does not write Confirmed or Canonical nodes.
pub(crate) fn extract(
    nodes: &[KnowledgeNode],
    edges: &[KnowledgeEdge],
    fragments: &[KnowledgeFragment],
    files: &BTreeMap<String, String>,
) -> Extracted {
    let classes = classes_of(nodes, fragments);
    let mut specs: Vec<Spec> = Vec::new();
    for class in &classes {
        if let Some(spec) = classify(class) {
            specs.push(spec);
        }
    }
    let mut contexts = context_specs(&classes, &specs, edges, files);
    let requirements = requirements_of(files);
    for spec in requirements.specs {
        if !specs.iter().any(|existing| {
            existing.context == spec.context
                && existing.kind == spec.kind
                && existing.name == spec.name
        }) {
            specs.push(spec);
        }
    }
    for context in requirements.contexts {
        if !contexts.iter().any(|existing| existing.key == context.key) {
            contexts.push(context);
        }
    }
    let mut invariants = invariants_of(&classes, &contexts, files);
    for invariant in requirements.invariants {
        if !invariants.iter().any(|existing| {
            existing.context == invariant.context && existing.name == invariant.name
        }) {
            invariants.push(invariant);
        }
    }

    let mut built: Vec<Built> = Vec::new();
    for context in &contexts {
        built.push(Built {
            node: make_node(context),
        });
    }
    for spec in &specs {
        built.push(Built {
            node: spec_node(spec),
        });
    }
    for invariant in &invariants {
        built.push(Built {
            node: invariant_node(invariant),
        });
    }
    let glossary = requirements.glossary;
    for spec in &specs {
        if !matches!(
            spec.kind,
            NodeKind::Entity | NodeKind::ValueObject | NodeKind::Aggregate
        ) {
            continue;
        }
        let glossed = glossary
            .iter()
            .any(|term| term.context == spec.context && term.name == spec.name);
        if glossed {
            continue;
        }
        built.push(Built {
            node: term_node(spec),
        });
    }
    for term in &glossary {
        built.push(Built {
            node: glossary_node(term),
        });
    }

    built.sort_by(|a, b| a.node.id.cmp(&b.node.id));
    built.dedup_by(|a, b| a.node.id == b.node.id);

    let mut edges_out = Vec::new();
    edges_out.extend(structure_edges(
        &built,
        &specs,
        &invariants,
        &requirements.value_links,
    ));
    edges_out.extend(call_edges(nodes, edges, &contexts));
    edges_out.extend(conflict_edges(&built));
    edges_out.sort_by(|a, b| a.id.cmp(&b.id));
    edges_out.dedup_by(|a, b| a.id == b.id);

    let mut findings = Vec::new();
    findings.extend(ambiguity_findings(&built));
    findings.extend(leak_findings(nodes, fragments, files));
    findings.extend(boundary_findings(&built, &invariants, files));
    findings.sort_by(|a, b| a.id.cmp(&b.id));
    findings.dedup_by(|a, b| a.id == b.id);

    let nodes_out: Vec<_> = built.into_iter().map(|item| item.node).collect();
    debug_assert!(nodes_out.iter().all(|node| {
        Lifecycle::parse(&node.status)
            .is_some_and(|status| !super::lifecycle::machine_status_forbidden(status))
    }));
    Extracted {
        nodes: nodes_out,
        edges: edges_out,
        findings,
    }
}

struct Spec {
    kind: NodeKind,
    name: String,
    context: String,
    summary: String,
    source: String,
    location: String,
    method: &'static str,
    confidence: u16,
    evidence: String,
    provenance: ProvenanceKind,
    status: Lifecycle,
    software_id: Option<String>,
    corrective: bool,
    /// Class or requirement text used to link value objects and aggregates.
    body: String,
}

struct ContextSpec {
    key: String,
    name: String,
    summary: String,
    source: String,
    location: String,
    method: &'static str,
    confidence: u16,
    evidence: String,
    provenance: ProvenanceKind,
    status: Lifecycle,
}

struct InvariantSpec {
    context: String,
    name: String,
    source: String,
    location: String,
    confidence: u16,
    evidence: String,
    provenance: ProvenanceKind,
    status: Lifecycle,
    method: &'static str,
}

struct TermSpec {
    context: String,
    name: String,
    summary: String,
    source: String,
    location: String,
}

struct Requirements {
    contexts: Vec<ContextSpec>,
    specs: Vec<Spec>,
    invariants: Vec<InvariantSpec>,
    glossary: Vec<TermSpec>,
    value_links: Vec<(String, String, String)>,
}

fn classes_of(nodes: &[KnowledgeNode], fragments: &[KnowledgeFragment]) -> Vec<ClassInfo> {
    let mut out = Vec::new();
    for node in nodes {
        if node.node_type != NodeKind::Class || !node.id.starts_with("software.") {
            continue;
        }
        if crate::id::is_test_path(&node.source_id) {
            continue;
        }
        let Some(context) = context_key(&node.scope) else {
            continue;
        };
        let (body, location) = fragment_text(fragments, &node.id);
        out.push(ClassInfo {
            id: node.id.clone(),
            name: node.name.clone(),
            scope: node.scope.clone(),
            source: node.source_id.clone(),
            summary: node.summary.clone(),
            body,
            location,
            context,
        });
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

fn classify(class: &ClassInfo) -> Option<Spec> {
    let explicit_entity = phrase(&class.summary, "entity") || phrase(&class.body, "entity");
    let explicit_vo = phrase(&class.summary, "value object") || phrase(&class.body, "value object");
    let explicit_aggregate =
        phrase(&class.summary, "aggregate root") || phrase(&class.body, "aggregate root");
    if class.name.starts_with("Base") && !explicit_entity && !explicit_vo && !explicit_aggregate {
        return None;
    }
    if class.name.ends_with("Controller") {
        return None;
    }
    let (kind, method, explicit) = if class.name.ends_with("AntiCorruptionLayer")
        || class.name == "Acl"
        || class.source.contains("/acl/")
    {
        (NodeKind::AntiCorruptionLayer, "naming-heuristic", false)
    } else if class.name.ends_with("Aggregate") || explicit_aggregate {
        (NodeKind::Aggregate, "naming-heuristic", explicit_aggregate)
    } else if class.name.ends_with("Repository") {
        (NodeKind::Repository, "naming-heuristic", false)
    } else if class.name.ends_with("Policy") {
        (NodeKind::Policy, "naming-heuristic", false)
    } else if class.name.ends_with("Event") {
        (NodeKind::DomainEvent, "naming-heuristic", false)
    } else if class.name.ends_with("Service") {
        (
            NodeKind::DomainService,
            "naming-heuristic",
            phrase(&class.summary, "domain service"),
        )
    } else if VALUE_NAMES.contains(&class.name.as_str()) || explicit_vo {
        (NodeKind::ValueObject, "naming-heuristic", explicit_vo)
    } else if explicit_entity || is_model_scope(&class.scope) {
        (NodeKind::Entity, "naming-heuristic", explicit_entity)
    } else {
        return None;
    };
    let mut signals = 1usize;
    if class.body.contains("_id") {
        signals += 1;
    }
    if explicit {
        signals += 1;
    }
    let corrective = kind == NodeKind::Policy
        && (phrase(&class.body, "repair")
            || phrase(&class.body, "reconcile")
            || phrase(&class.body, "periodic"));
    let evidence = if explicit {
        format!(
            "docstring or body of {} names the role; class {} in {}",
            class.source, class.name, class.scope
        )
    } else if kind == NodeKind::Entity && class.body.contains("_id") {
        format!(
            "model class {} has an identity field in {}",
            class.name, class.source
        )
    } else {
        format!(
            "name {} in {} matches a {} pattern",
            class.name,
            class.source,
            kind.as_str()
        )
    };
    Some(Spec {
        kind,
        name: class.name.clone(),
        context: class.context.clone(),
        summary: if class.summary.is_empty() {
            format!("{} {}", kind.as_str(), class.name)
        } else {
            class.summary.clone()
        },
        source: class.source.clone(),
        location: class.location.clone(),
        method,
        confidence: confidence(signals, explicit),
        evidence,
        provenance: ProvenanceKind::Inferred,
        status: Origin::Heuristic.initial(),
        software_id: Some(class.id.clone()),
        corrective,
        body: class.body.clone(),
    })
}

fn context_specs(
    classes: &[ClassInfo],
    specs: &[Spec],
    edges: &[KnowledgeEdge],
    files: &BTreeMap<String, String>,
) -> Vec<ContextSpec> {
    let mut keys: BTreeSet<String> = classes.iter().map(|class| class.context.clone()).collect();
    keys.extend(specs.iter().map(|spec| spec.context.clone()));
    let mut out = Vec::new();
    for key in keys {
        let leaf = key.rsplit('.').next().unwrap_or(key.as_str());
        let name = pascal(leaf);
        let related: Vec<&ClassInfo> = classes
            .iter()
            .filter(|class| class.context == key)
            .collect();
        let mut listed: Vec<&str> = specs
            .iter()
            .filter(|spec| spec.context == key)
            .map(|spec| spec.name.as_str())
            .collect();
        let mut signals = 0usize;
        if specs.iter().any(|spec| {
            spec.context == key
                && matches!(
                    spec.kind,
                    NodeKind::Entity | NodeKind::ValueObject | NodeKind::Aggregate
                )
        }) {
            signals += 1;
        }
        if specs
            .iter()
            .any(|spec| spec.context == key && spec.kind == NodeKind::DomainService)
        {
            signals += 1;
        }
        if specs
            .iter()
            .any(|spec| spec.context == key && spec.kind == NodeKind::Repository)
        {
            signals += 1;
        }
        if files
            .keys()
            .any(|path| path_has_token(path, leaf) && crate::id::is_test_path(path))
        {
            signals += 1;
        }
        let class_ids: BTreeSet<&str> = related.iter().map(|class| class.id.as_str()).collect();
        if edges.iter().any(|edge| {
            edge.relation == RelationType::References && class_ids.contains(edge.to_id.as_str())
        }) {
            signals += 1;
        }
        let anchor = specs
            .iter()
            .filter(|spec| spec.context == key)
            .min_by(|left, right| {
                left.source
                    .cmp(&right.source)
                    .then(left.name.cmp(&right.name))
            });
        let source = anchor.map(|spec| spec.source.clone()).unwrap_or_default();
        let location = anchor
            .map(|spec| spec.location.clone())
            .unwrap_or_else(|| line_span(1, 1));
        listed.sort_unstable();
        listed.dedup();
        out.push(ContextSpec {
            key: key.clone(),
            name,
            summary: format!("Package {key}"),
            source,
            location,
            method: "package-heuristic",
            confidence: confidence(signals.max(1), false),
            evidence: format!("classes: {}", listed.join(", ")),
            provenance: ProvenanceKind::Inferred,
            status: Origin::Heuristic.initial(),
        });
    }
    out
}

fn invariants_of(
    classes: &[ClassInfo],
    contexts: &[ContextSpec],
    files: &BTreeMap<String, String>,
) -> Vec<InvariantSpec> {
    let keys: Vec<&str> = contexts
        .iter()
        .map(|context| context.key.as_str())
        .collect();
    let mut found: BTreeMap<(String, String), InvariantSpec> = BTreeMap::new();
    for (path, text) in files {
        if !text.contains("amount > 0") {
            continue;
        }
        let mut hit = BTreeSet::new();
        for class in classes {
            if &class.source == path {
                hit.insert(class.context.clone());
            }
        }
        if hit.is_empty() {
            for key in &keys {
                let leaf = key.rsplit('.').next().unwrap_or(key);
                if path_has_token(path, leaf) {
                    hit.insert((*key).to_owned());
                }
            }
        }
        let line = line_number(text, "amount > 0");
        for context in hit {
            let entry = found
                .entry((context.clone(), "amount > 0".to_owned()))
                .or_insert_with(|| InvariantSpec {
                    context,
                    name: "amount > 0".to_owned(),
                    source: path.clone(),
                    location: line_span(line, line),
                    confidence: 700,
                    evidence: format!("literal \"amount > 0\" in {path}"),
                    provenance: ProvenanceKind::Inferred,
                    status: Origin::Heuristic.initial(),
                    method: "invariant-text",
                });
            if !entry.evidence.contains(path) {
                entry.evidence.push_str(", ");
                entry.evidence.push_str(path);
                entry.confidence = 800;
            }
        }
    }
    found.into_values().collect()
}

fn requirements_of(files: &BTreeMap<String, String>) -> Requirements {
    let mut out = Requirements {
        contexts: Vec::new(),
        specs: Vec::new(),
        invariants: Vec::new(),
        glossary: Vec::new(),
        value_links: Vec::new(),
    };
    for (path, text) in files {
        if !path.ends_with(".md") {
            continue;
        }
        let mut current: Option<String> = None;
        let mut glossary = false;
        for (index, line) in text.lines().enumerate() {
            let trimmed = line.trim();
            let line_no = u32::try_from(index + 1).unwrap_or(1);
            if trimmed.starts_with('#') {
                glossary = phrase(trimmed, "ubiquitous language");
                continue;
            }
            if glossary {
                if let Some(term) = glossary_bullet(trimmed)
                    && let Some(context) = current.clone()
                {
                    out.glossary.push(TermSpec {
                        context,
                        name: term.0,
                        summary: term.1,
                        source: path.clone(),
                        location: line_span(line_no, line_no),
                    });
                }
                continue;
            }
            if let Some(name) = sentence(trimmed, " is a bounded context") {
                current = Some(name.to_ascii_lowercase());
                out.contexts.push(ContextSpec {
                    key: name.to_ascii_lowercase(),
                    name: name.to_owned(),
                    summary: format!("Declared in {path}"),
                    source: path.clone(),
                    location: line_span(line_no, line_no),
                    method: "requirement",
                    confidence: 700,
                    evidence: format!("requirements sentence in {path}"),
                    provenance: ProvenanceKind::Declared,
                    status: Origin::Requirement.initial(),
                });
                continue;
            }
            if let Some((name, context)) = in_context(trimmed, " is an entity in ") {
                out.specs.push(requirement_spec(
                    NodeKind::Entity,
                    name,
                    context,
                    path,
                    line_no,
                ));
                current = Some(context.to_ascii_lowercase());
                continue;
            }
            if let Some((name, context)) = in_context(trimmed, " is a value object in ") {
                out.specs.push(requirement_spec(
                    NodeKind::ValueObject,
                    name,
                    context,
                    path,
                    line_no,
                ));
                continue;
            }
            if let Some((name, context)) = in_context(trimmed, " is an aggregate in ") {
                out.specs.push(requirement_spec(
                    NodeKind::Aggregate,
                    name,
                    context,
                    path,
                    line_no,
                ));
                continue;
            }
            if let Some((name, context)) = in_context(trimmed, " is a domain service in ") {
                out.specs.push(requirement_spec(
                    NodeKind::DomainService,
                    name,
                    context,
                    path,
                    line_no,
                ));
                continue;
            }
            if let Some((name, context)) = in_context(trimmed, " is a repository in ") {
                out.specs.push(requirement_spec(
                    NodeKind::Repository,
                    name,
                    context,
                    path,
                    line_no,
                ));
                continue;
            }
            if let Some((name, context)) = in_context(trimmed, " is a policy in ") {
                out.specs.push(requirement_spec(
                    NodeKind::Policy,
                    name,
                    context,
                    path,
                    line_no,
                ));
                continue;
            }
            if let Some((name, context)) = in_context(trimmed, " is a domain event in ") {
                out.specs.push(requirement_spec(
                    NodeKind::DomainEvent,
                    name,
                    context,
                    path,
                    line_no,
                ));
                continue;
            }
            if let Some((owner, value)) = has_value(trimmed)
                && let Some(context) = current.clone()
            {
                out.value_links
                    .push((context, owner.to_owned(), value.to_owned()));
            }
        }
        if text.contains("amount > 0")
            && let Some(context) = current.clone()
        {
            let line = line_number(text, "amount > 0");
            out.invariants.push(InvariantSpec {
                context,
                name: "amount > 0".to_owned(),
                source: path.clone(),
                location: line_span(line, line),
                confidence: 700,
                evidence: format!("literal \"amount > 0\" in {path}"),
                provenance: ProvenanceKind::Declared,
                status: Origin::Requirement.initial(),
                method: "requirement",
            });
        }
    }
    out
}

fn requirement_spec(kind: NodeKind, name: &str, context: &str, path: &str, line: u32) -> Spec {
    Spec {
        kind,
        name: name.to_owned(),
        context: context.to_ascii_lowercase(),
        summary: format!("Declared {name} in {path}"),
        source: path.to_owned(),
        location: line_span(line, line),
        method: "requirement",
        confidence: 700,
        evidence: format!("requirements sentence in {path}"),
        provenance: ProvenanceKind::Declared,
        status: Origin::Requirement.initial(),
        software_id: None,
        corrective: false,
        body: String::new(),
    }
}

fn structure_edges(
    built: &[Built],
    specs: &[Spec],
    invariants: &[InvariantSpec],
    value_links: &[(String, String, String)],
) -> Vec<KnowledgeEdge> {
    let mut edges = Vec::new();
    for node in built {
        if node.node.node_type == NodeKind::BoundedContext {
            continue;
        }
        let context_id = context_node_id(&node.node.scope);
        if built.iter().any(|item| item.node.id == context_id) {
            edges.push(link(
                &node.node.id,
                RelationType::BelongsTo,
                &context_id,
                &node.node,
                "package-heuristic",
            ));
        }
        if let Some(concept) = node.node.payload.get("concept") {
            edges.push(link(
                &node.node.id,
                RelationType::DerivedFrom,
                concept,
                &node.node,
                "term-from-concept",
            ));
        }
    }
    for spec in specs {
        let id = domain_id(&spec.context, spec.kind, &spec.name);
        let context_id = context_node_id(&spec.context);
        let Some(node) = built.iter().find(|item| item.node.id == id) else {
            continue;
        };
        if built.iter().any(|item| item.node.id == context_id) {
            let relation = match spec.kind {
                NodeKind::Entity => Some(RelationType::HasEntity),
                NodeKind::ValueObject => Some(RelationType::HasValueObject),
                NodeKind::Aggregate => Some(RelationType::HasAggregate),
                _ => None,
            };
            if let Some(relation) = relation {
                edges.push(link(&context_id, relation, &id, &node.node, spec.method));
            }
        }
        if let Some(software) = &spec.software_id {
            edges.push(link(
                &id,
                RelationType::DerivedFrom,
                software,
                &node.node,
                spec.method,
            ));
        }
        if spec.corrective && built.iter().any(|item| item.node.id == context_id) {
            edges.push(link(
                &id,
                RelationType::Governs,
                &context_id,
                &node.node,
                spec.method,
            ));
        }
        if spec.kind == NodeKind::AntiCorruptionLayer {
            for other in specs {
                if other.context == spec.context
                    || other.kind != NodeKind::Entity
                    || !whole_word(&spec.body, &other.name)
                {
                    continue;
                }
                let other_context = context_node_id(&other.context);
                edges.push(link(
                    &id,
                    RelationType::AntiCorrupts,
                    &other_context,
                    &node.node,
                    spec.method,
                ));
            }
        }
        if spec.kind == NodeKind::Entity {
            for value in specs.iter().filter(|other| {
                other.kind == NodeKind::ValueObject && other.context == spec.context
            }) {
                if !whole_word(&spec.body, &value.name) {
                    continue;
                }
                let target = domain_id(&value.context, value.kind, &value.name);
                edges.push(link(
                    &id,
                    RelationType::HasValueObject,
                    &target,
                    &node.node,
                    "naming-heuristic",
                ));
            }
        }
        if spec.kind == NodeKind::Aggregate {
            for entity in specs
                .iter()
                .filter(|other| other.kind == NodeKind::Entity && other.context == spec.context)
            {
                if !whole_word(&spec.body, &entity.name) {
                    continue;
                }
                let target = domain_id(&entity.context, entity.kind, &entity.name);
                edges.push(link(
                    &id,
                    RelationType::Contains,
                    &target,
                    &node.node,
                    "naming-heuristic",
                ));
            }
        }
    }
    for (context, owner, value) in value_links {
        let from = domain_id(context, NodeKind::Entity, owner);
        let to = domain_id(context, NodeKind::ValueObject, value);
        if let Some(node) = built.iter().find(|item| item.node.id == from)
            && built.iter().any(|item| item.node.id == to)
        {
            edges.push(link(
                &from,
                RelationType::HasValueObject,
                &to,
                &node.node,
                "requirement",
            ));
        }
    }
    for invariant in invariants {
        let id = format!(
            "domain.{}.invariant.{}",
            invariant.context,
            slug(&invariant.name)
        );
        let Some(node) = built.iter().find(|item| item.node.id == id) else {
            continue;
        };
        for spec in specs
            .iter()
            .filter(|spec| spec.context == invariant.context)
        {
            let constrains = match spec.kind {
                NodeKind::Aggregate => true,
                NodeKind::Entity => {
                    spec.source == invariant.source || spec.body.contains("amount > 0")
                }
                _ => false,
            };
            if !constrains {
                continue;
            }
            let from = domain_id(&spec.context, spec.kind, &spec.name);
            edges.push(link(
                &from,
                RelationType::ConstrainedBy,
                &id,
                &node.node,
                invariant.method,
            ));
        }
    }
    edges
}

fn context_node_id(context: &str) -> String {
    let leaf = context.rsplit('.').next().unwrap_or(context);
    domain_id(context, NodeKind::BoundedContext, &pascal(leaf))
}

fn call_edges(
    nodes: &[KnowledgeNode],
    edges: &[KnowledgeEdge],
    contexts: &[ContextSpec],
) -> Vec<KnowledgeEdge> {
    let scope_of: BTreeMap<&str, &str> = nodes
        .iter()
        .map(|node| (node.id.as_str(), node.scope.as_str()))
        .collect();
    let mut pairs: BTreeMap<(String, String), String> = BTreeMap::new();
    for edge in edges {
        if edge.relation != RelationType::Calls {
            continue;
        }
        let Some(from_scope) = scope_of.get(edge.from_id.as_str()) else {
            continue;
        };
        let Some(to_scope) = scope_of.get(edge.to_id.as_str()) else {
            continue;
        };
        let Some(from_key) = context_key(from_scope) else {
            continue;
        };
        let Some(to_key) = context_key(to_scope) else {
            continue;
        };
        if from_key != to_key {
            pairs
                .entry((from_key, to_key))
                .or_insert_with(|| edge.source_id.clone());
        }
    }
    let mut out = Vec::new();
    for ((downstream, upstream), source) in pairs {
        if !contexts.iter().any(|context| context.key == downstream)
            || !contexts.iter().any(|context| context.key == upstream)
        {
            continue;
        }
        let from = domain_id(
            &downstream,
            NodeKind::BoundedContext,
            &pascal(downstream.rsplit('.').next().unwrap_or(&downstream)),
        );
        let to = domain_id(
            &upstream,
            NodeKind::BoundedContext,
            &pascal(upstream.rsplit('.').next().unwrap_or(&upstream)),
        );
        let mut metadata = BTreeMap::new();
        metadata.insert("downstream".to_owned(), downstream.clone());
        metadata.insert("upstream".to_owned(), upstream.clone());
        metadata.insert("direction".to_owned(), format!("{downstream}->{upstream}"));
        let provenance = domain_provenance(ProvenanceKind::Inferred, &source, "", "context-call");
        out.push(KnowledgeEdge {
            id: edge_id(&from, RelationType::CrossContextTo, &to),
            from_id: from,
            relation: RelationType::CrossContextTo,
            to_id: to,
            confidence_millis: 650,
            provenance,
            metadata,
            source_id: source,
        });
    }
    out
}

fn conflict_edges(built: &[Built]) -> Vec<KnowledgeEdge> {
    let terms: Vec<&KnowledgeNode> = built
        .iter()
        .map(|item| &item.node)
        .filter(|node| node.node_type == NodeKind::Terminology)
        .collect();
    let mut edges = Vec::new();
    for group in SYNONYMS {
        let mut members: Vec<&KnowledgeNode> = terms
            .iter()
            .copied()
            .filter(|node| group.contains(&normalize(&node.name).as_str()))
            .collect();
        members.sort_by(|a, b| a.id.cmp(&b.id));
        members.dedup_by(|a, b| a.id == b.id);
        if members.len() < 2 {
            continue;
        }
        for left in 0..members.len() {
            for right in (left + 1)..members.len() {
                let from = &members[left];
                let to = &members[right];
                edges.push(link(
                    &from.id,
                    RelationType::ConflictsWith,
                    &to.id,
                    from,
                    "synonym-set",
                ));
            }
        }
    }
    edges
}

fn ambiguity_findings(built: &[Built]) -> Vec<Finding> {
    let terms: Vec<&KnowledgeNode> = built
        .iter()
        .map(|item| &item.node)
        .filter(|node| node.node_type == NodeKind::Terminology)
        .collect();
    let mut findings = Vec::new();
    for group in SYNONYMS {
        let mut members: Vec<&KnowledgeNode> = terms
            .iter()
            .copied()
            .filter(|node| group.contains(&normalize(&node.name).as_str()))
            .collect();
        members.sort_by(|a, b| a.name.cmp(&b.name));
        members.dedup_by(|a, b| a.id == b.id);
        if members.len() < 2 {
            continue;
        }
        let mut names: Vec<&str> = members.iter().map(|node| node.name.as_str()).collect();
        names.sort_unstable();
        names.dedup();
        let mut subjects: Vec<String> = members.iter().map(|node| node.id.clone()).collect();
        subjects.sort();
        subjects.dedup();
        let slug = names
            .iter()
            .map(|name| normalize(name))
            .collect::<Vec<_>>()
            .join("-");
        let source = members
            .first()
            .map(|node| node.provenance.source.clone())
            .unwrap_or_default();
        findings.push(Finding {
            id: format!("finding.terminology_ambiguity.{slug}"),
            kind: "terminology_ambiguity".to_owned(),
            summary: format!(
                "{} are near-duplicate terms and were not merged",
                names.join(", ")
            ),
            provenance: domain_provenance(ProvenanceKind::Inferred, &source, "", "synonym-set"),
            confidence_millis: CONFIDENCE_INFERRED,
            evidence: format!("synonym set {}", group.join(", ")),
            subjects,
        });
    }
    findings
}

fn leak_findings(
    nodes: &[KnowledgeNode],
    fragments: &[KnowledgeFragment],
    files: &BTreeMap<String, String>,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    for node in nodes {
        if node.node_type != NodeKind::Import {
            continue;
        }
        if !is_domain_core(&node.source_id) {
            continue;
        }
        let Some(target) = node.payload.get("target") else {
            continue;
        };
        let root = target.split('.').next().unwrap_or(target);
        if !INFRA.contains(&root) {
            continue;
        }
        if adr_allows(files, root) {
            continue;
        }
        let (_, location) = fragment_text(fragments, &node.id);
        findings.push(Finding {
            id: format!("finding.hexagonal_leak.{}.{}", slug(&node.source_id), root),
            kind: "hexagonal_leak".to_owned(),
            summary: format!("{root} is imported from domain module {}", node.source_id),
            provenance: domain_provenance(
                ProvenanceKind::Inferred,
                &node.source_id,
                &location,
                "import-leak",
            ),
            confidence_millis: 800,
            evidence: format!("import {target} in {}", node.source_id),
            subjects: vec![node.id.clone()],
        });
    }
    findings
}

fn boundary_findings(
    built: &[Built],
    invariants: &[InvariantSpec],
    files: &BTreeMap<String, String>,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    let contexts: BTreeSet<&str> = invariants
        .iter()
        .map(|item| item.context.as_str())
        .collect();
    for context in contexts {
        let has_aggregate = built
            .iter()
            .any(|item| item.node.node_type == NodeKind::Aggregate && item.node.scope == context);
        if !has_aggregate {
            continue;
        }
        for (path, text) in files {
            if !path.contains("controller") && !path.contains("adapter") {
                continue;
            }
            if !text.contains(".amount =") {
                continue;
            }
            findings.push(Finding {
                id: format!("finding.aggregate_boundary.{context}.{}", slug(path)),
                kind: "aggregate_boundary".to_owned(),
                summary: format!("{path} assigns amount outside the {context} aggregate"),
                provenance: domain_provenance(
                    ProvenanceKind::Inferred,
                    path,
                    &line_span(
                        line_number(text, ".amount ="),
                        line_number(text, ".amount ="),
                    ),
                    "aggregate-boundary",
                ),
                confidence_millis: 600,
                evidence: format!(
                    "assignment in {path} while {context} has an aggregate and amount > 0"
                ),
                subjects: vec![domain_id(
                    context,
                    NodeKind::BoundedContext,
                    &pascal(context.rsplit('.').next().unwrap_or(context)),
                )],
            });
        }
    }
    findings
}

fn make_node(context: &ContextSpec) -> KnowledgeNode {
    finish(KnowledgeNode {
        id: domain_id(&context.key, NodeKind::BoundedContext, &context.name),
        node_type: NodeKind::BoundedContext,
        name: context.name.clone(),
        scope: context.key.clone(),
        summary: context.summary.clone(),
        status: context.status.as_str().to_owned(),
        provenance: domain_provenance(
            context.provenance,
            &context.source,
            &context.location,
            context.method,
        ),
        confidence_millis: context.confidence,
        version: NODE_VERSION,
        fingerprint: String::new(),
        source_refs: ref_list(&context.source),
        retrieval_metadata: BTreeMap::new(),
        payload: payload_evidence(&context.evidence),
        source_id: context.source.clone(),
        parent: None,
    })
}

fn spec_node(spec: &Spec) -> KnowledgeNode {
    let mut payload = payload_evidence(&spec.evidence);
    if spec.corrective {
        payload.insert("corrective".to_owned(), "true".to_owned());
    }
    if let Some(software) = &spec.software_id {
        payload.insert("software_id".to_owned(), software.clone());
    }
    finish(KnowledgeNode {
        id: domain_id(&spec.context, spec.kind, &spec.name),
        node_type: spec.kind,
        name: spec.name.clone(),
        scope: spec.context.clone(),
        summary: spec.summary.clone(),
        status: spec.status.as_str().to_owned(),
        provenance: domain_provenance(spec.provenance, &spec.source, &spec.location, spec.method),
        confidence_millis: spec.confidence,
        version: NODE_VERSION,
        fingerprint: String::new(),
        source_refs: ref_list(&spec.source),
        retrieval_metadata: BTreeMap::new(),
        payload,
        source_id: spec.source.clone(),
        parent: None,
    })
}

fn invariant_node(invariant: &InvariantSpec) -> KnowledgeNode {
    finish(KnowledgeNode {
        id: format!(
            "domain.{}.invariant.{}",
            invariant.context,
            slug(&invariant.name)
        ),
        node_type: NodeKind::Invariant,
        name: invariant.name.clone(),
        scope: invariant.context.clone(),
        summary: format!("Invariant {}", invariant.name),
        status: invariant.status.as_str().to_owned(),
        provenance: domain_provenance(
            invariant.provenance,
            &invariant.source,
            &invariant.location,
            invariant.method,
        ),
        confidence_millis: invariant.confidence,
        version: NODE_VERSION,
        fingerprint: String::new(),
        source_refs: ref_list(&invariant.source),
        retrieval_metadata: BTreeMap::new(),
        payload: payload_evidence(&invariant.evidence),
        source_id: invariant.source.clone(),
        parent: None,
    })
}

fn term_node(spec: &Spec) -> KnowledgeNode {
    let mut payload = payload_evidence(&format!("term taken from {}", spec.kind.as_str()));
    payload.insert(
        "concept".to_owned(),
        domain_id(&spec.context, spec.kind, &spec.name),
    );
    finish(KnowledgeNode {
        id: domain_id(&spec.context, NodeKind::Terminology, &spec.name),
        node_type: NodeKind::Terminology,
        name: spec.name.clone(),
        scope: spec.context.clone(),
        summary: format!("Term {}", spec.name),
        status: Origin::Heuristic.initial().as_str().to_owned(),
        provenance: domain_provenance(
            ProvenanceKind::Inferred,
            &spec.source,
            &spec.location,
            "term-from-concept",
        ),
        confidence_millis: 600,
        version: NODE_VERSION,
        fingerprint: String::new(),
        source_refs: ref_list(&spec.source),
        retrieval_metadata: BTreeMap::new(),
        payload,
        source_id: spec.source.clone(),
        parent: None,
    })
}

fn glossary_node(term: &TermSpec) -> KnowledgeNode {
    finish(KnowledgeNode {
        id: domain_id(&term.context, NodeKind::Terminology, &term.name),
        node_type: NodeKind::Terminology,
        name: term.name.clone(),
        scope: term.context.clone(),
        summary: term.summary.clone(),
        status: Origin::Glossary.initial().as_str().to_owned(),
        provenance: domain_provenance(
            ProvenanceKind::Declared,
            &term.source,
            &term.location,
            "glossary",
        ),
        confidence_millis: 900,
        version: NODE_VERSION,
        fingerprint: String::new(),
        source_refs: ref_list(&term.source),
        retrieval_metadata: BTreeMap::new(),
        payload: payload_evidence(&format!("glossary entry in {}", term.source)),
        source_id: term.source.clone(),
        parent: None,
    })
}

fn finish(mut node: KnowledgeNode) -> KnowledgeNode {
    node.fingerprint = fingerprint(&node);
    node
}

pub(crate) fn fingerprint(node: &KnowledgeNode) -> String {
    let evidence = node
        .payload
        .get("evidence")
        .map(String::as_str)
        .unwrap_or("");
    crate::model::node_fingerprint(
        node.node_type,
        &node.name,
        &node.scope,
        node.provenance.kind,
        &format!("{}\n{}\n{evidence}", node.summary, node.status),
    )
}

fn link(
    from: &str,
    relation: RelationType,
    to: &str,
    sample: &KnowledgeNode,
    method: &str,
) -> KnowledgeEdge {
    KnowledgeEdge {
        id: edge_id(from, relation, to),
        from_id: from.to_owned(),
        relation,
        to_id: to.to_owned(),
        confidence_millis: sample.confidence_millis,
        provenance: domain_provenance(
            sample.provenance.kind,
            &sample.provenance.source,
            &sample.provenance.location,
            method,
        ),
        metadata: BTreeMap::new(),
        source_id: sample.source_id.clone(),
    }
}

fn domain_provenance(
    kind: ProvenanceKind,
    source: &str,
    location: &str,
    method: &str,
) -> Provenance {
    Provenance {
        kind,
        source: source.to_owned(),
        location: if location.is_empty() {
            line_span(1, 1)
        } else {
            location.to_owned()
        },
        extractor: "domain".to_owned(),
        extractor_version: "1".to_owned(),
        method: method.to_owned(),
    }
}

fn domain_id(context: &str, kind: NodeKind, name: &str) -> String {
    format!("domain.{context}.{}.{name}", kind.as_str())
}

fn payload_evidence(evidence: &str) -> BTreeMap<String, String> {
    let mut payload = BTreeMap::new();
    payload.insert("evidence".to_owned(), evidence.to_owned());
    payload
}

fn ref_list(source: &str) -> Vec<String> {
    if source.is_empty() {
        Vec::new()
    } else {
        vec![source.to_owned()]
    }
}

fn context_key(scope: &str) -> Option<String> {
    let mut parts: Vec<&str> = scope.split('.').filter(|part| !part.is_empty()).collect();
    while parts
        .last()
        .copied()
        .is_some_and(|part| ROLE.contains(&part))
    {
        parts.pop();
    }
    if parts
        .first()
        .copied()
        .is_some_and(|part| PACKAGE_ROOTS.contains(&part))
    {
        parts.remove(0);
    }
    if parts.is_empty() || (parts.len() == 1 && GENERIC.contains(&parts[0])) {
        return None;
    }
    Some(parts.join("."))
}

fn is_model_scope(scope: &str) -> bool {
    scope
        .rsplit('.')
        .next()
        .is_some_and(|leaf| matches!(leaf, "models" | "model" | "entities" | "entity" | "domain"))
}

fn is_domain_core(path: &str) -> bool {
    if path.contains("/adapter") || path.contains("/infra") {
        return false;
    }
    if path.contains("/domain/") {
        return true;
    }
    let file = path.rsplit('/').next().unwrap_or(path);
    let stem = file.rsplit_once('.').map(|(stem, _)| stem).unwrap_or(file);
    matches!(stem, "models" | "model" | "entities" | "entity")
}

fn pascal(segment: &str) -> String {
    let mut out = String::new();
    let mut cap = true;
    for ch in segment.chars() {
        if ch == '_' || ch == '-' || ch == '.' {
            cap = true;
            continue;
        }
        if cap {
            out.extend(ch.to_uppercase());
            cap = false;
        } else {
            out.push(ch);
        }
    }
    out
}

fn confidence(signals: usize, explicit: bool) -> u16 {
    if explicit {
        return 850;
    }
    match signals {
        0 | 1 => 550,
        2 => 700,
        _ => 800,
    }
}

fn phrase(text: &str, needle: &str) -> bool {
    text.to_ascii_lowercase().contains(needle)
}

fn whole_word(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return false;
    }
    haystack.match_indices(needle).any(|(index, _)| {
        let before_ok = haystack[..index]
            .chars()
            .next_back()
            .is_none_or(|ch| !ch.is_ascii_alphanumeric() && ch != '_');
        let after_ok = haystack[index + needle.len()..]
            .chars()
            .next()
            .is_none_or(|ch| !ch.is_ascii_alphanumeric() && ch != '_');
        before_ok && after_ok
    })
}

fn normalize(name: &str) -> String {
    name.chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(|ch| ch.to_lowercase())
        .collect()
}

fn slug(value: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for ch in value.chars() {
        let mapped = match ch {
            '>' => "gt",
            '<' => "lt",
            '=' => "eq",
            _ => "",
        };
        if !mapped.is_empty() {
            if !out.is_empty() && !prev_dash {
                out.push('-');
            }
            out.push_str(mapped);
            out.push('-');
            prev_dash = true;
            continue;
        }
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash && !out.is_empty() {
            out.push('-');
            prev_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

fn line_number(text: &str, needle: &str) -> u32 {
    text.lines()
        .position(|line| line.contains(needle))
        .and_then(|index| u32::try_from(index + 1).ok())
        .unwrap_or(1)
}

fn path_has_token(path: &str, token: &str) -> bool {
    path.split(['/', '.', '_', '-']).any(|part| part == token)
}

fn fragment_text(fragments: &[KnowledgeFragment], node_id: &str) -> (String, String) {
    let mut body = String::new();
    let mut location = String::new();
    for fragment in fragments
        .iter()
        .filter(|fragment| fragment.node_id == node_id)
    {
        if location.is_empty() {
            location.clone_from(&fragment.location);
        }
        body.push_str(&fragment.content);
        body.push('\n');
    }
    if location.is_empty() {
        location = line_span(1, 1);
    }
    (body, location)
}

fn sentence<'a>(line: &'a str, suffix: &str) -> Option<&'a str> {
    let stripped = line.trim().trim_end_matches('.').trim();
    let name = stripped.strip_suffix(suffix.trim())?.trim();
    if name.chars().next()?.is_ascii_uppercase()
        && name.chars().all(|ch| ch.is_ascii_alphanumeric())
    {
        Some(name)
    } else {
        None
    }
}

fn in_context<'a>(line: &'a str, marker: &str) -> Option<(&'a str, &'a str)> {
    let stripped = line.trim().trim_end_matches('.').trim();
    let (left, right) = stripped.split_once(marker)?;
    let name = left.trim();
    let context = right.trim();
    if name.chars().next()?.is_ascii_uppercase()
        && context.chars().next()?.is_ascii_uppercase()
        && name.chars().all(|ch| ch.is_ascii_alphanumeric())
        && context.chars().all(|ch| ch.is_ascii_alphanumeric())
    {
        Some((name, context))
    } else {
        None
    }
}

fn has_value(line: &str) -> Option<(&str, &str)> {
    let stripped = line.trim().trim_end_matches('.').trim();
    let (left, right) = stripped.split_once(" has value object ")?;
    let owner = left.trim();
    let value = right.trim();
    if owner.chars().all(|ch| ch.is_ascii_alphanumeric())
        && value.chars().all(|ch| ch.is_ascii_alphanumeric())
        && owner.chars().next()?.is_ascii_uppercase()
        && value.chars().next()?.is_ascii_uppercase()
    {
        Some((owner, value))
    } else {
        None
    }
}

fn glossary_bullet(line: &str) -> Option<(String, String)> {
    let rest = line.strip_prefix("- ")?;
    let (name, summary) = rest.split_once(':')?;
    let name = name.trim();
    if name.chars().next()?.is_ascii_uppercase()
        && name.chars().all(|ch| ch.is_ascii_alphanumeric())
    {
        Some((name.to_owned(), summary.trim().to_owned()))
    } else {
        None
    }
}

fn adr_allows(files: &BTreeMap<String, String>, root: &str) -> bool {
    let root = root.to_ascii_lowercase();
    files.iter().any(|(path, text)| {
        if !is_adr(path) {
            return false;
        }
        let lower = text.to_ascii_lowercase();
        lower.contains(&root)
            && (lower.contains("exception")
                || lower.contains("allowed")
                || lower.contains("waived")
                || lower.contains("permitted"))
    })
}
