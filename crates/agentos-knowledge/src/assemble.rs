//! Turn extractor output into nodes, edges, and fragments.
//!
//! Cross-file links are mechanical: a call is linked only when the source names the
//! receiver's type (or the callee is a bare name) and exactly one symbol matches.
//! Document mentions are inferred and stay inferred.

use std::collections::{BTreeMap, BTreeSet};

use crate::docs::{self, DocExtract};
use crate::error::KnowledgeError;
use crate::extract::RawSymbol;
use crate::id::{self, ModulePath};
use crate::model::{
    CONFIDENCE_DECLARED, CONFIDENCE_INFERRED, CONFIDENCE_RESOLVED, CONFIDENCE_STRUCTURAL,
    KnowledgeEdge, KnowledgeFragment, KnowledgeNode, NODE_VERSION, NodeKind, Provenance,
    ProvenanceKind, RelationType, edge_id, fragment, line_span, node_fingerprint,
};

/// One file's contribution to the graph, before or after cross-file linking.
#[derive(Debug, Clone)]
pub struct Draft {
    /// Relative path.
    pub source_id: String,
    /// `python`, `typescript`, `javascript`, `markdown`, or empty when the file is only fingerprinted.
    pub language: String,
    /// Content hash.
    pub fingerprint: String,
    /// Byte length.
    pub byte_len: i64,
    /// Nodes extracted from this file.
    pub nodes: Vec<KnowledgeNode>,
    /// Edges asserted by this file.
    pub edges: Vec<KnowledgeEdge>,
    /// Fragments of this file's nodes.
    pub fragments: Vec<KnowledgeFragment>,
    /// Symbols kept so calls can be resolved after every file is known.
    pub raw: Vec<RawSymbol>,
    /// Set for code files.
    pub module: Option<ModulePath>,
}

/// A symbol already stored, used to resolve links without re-parsing unchanged files.
#[derive(Debug, Clone)]
pub struct StoredSymbol {
    /// Logical id.
    pub id: String,
    /// Node kind.
    pub kind: NodeKind,
    /// Local name.
    pub name: String,
    /// Enclosing type, for methods.
    pub parent: Option<String>,
    /// Module path, or document path.
    pub scope: String,
}

/// Builds a draft from a code extraction. Cross-file edges are added by [`link`].
pub fn draft_code(
    relative: &str,
    fingerprint: &str,
    byte_len: i64,
    extractor: &str,
    extractor_version: &str,
    symbols: Vec<RawSymbol>,
) -> Draft {
    let module = id::module_path(relative);
    let test_file = id::is_test_path(relative);
    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    let mut fragments = Vec::new();
    let mut seen_imports = BTreeSet::new();
    let mut seen_ids = BTreeSet::new();

    let file_id = id::container_id(&module, "file");
    let module_id = id::container_id(&module, "module");
    nodes.push(container_node(
        file_id.clone(),
        NodeKind::File,
        file_name(relative),
        &module.module,
        relative,
        extractor,
        extractor_version,
    ));
    nodes.push(container_node(
        module_id.clone(),
        NodeKind::Module,
        &module.module,
        &module.module,
        relative,
        extractor,
        extractor_version,
    ));
    edges.push(structural_edge(
        &file_id,
        RelationType::Contains,
        &module_id,
        relative,
        extractor,
        extractor_version,
        "L1-L1",
    ));
    seen_ids.insert(file_id);
    seen_ids.insert(module_id.clone());

    for symbol in &symbols {
        if symbol.kind == NodeKind::Import {
            let target = symbol
                .import_target
                .clone()
                .unwrap_or_else(|| symbol.name.clone());
            if !seen_imports.insert(target) {
                continue;
            }
        }
        let kind = reclassify(symbol, test_file);
        let qualifier = match (&symbol.parent, kind) {
            (Some(parent), NodeKind::Method | NodeKind::Test) => {
                format!("{parent}.{}", symbol.name)
            }
            _ => symbol.name.clone(),
        };
        let id = id::symbol_id(&module, kind.as_str(), &qualifier);
        if !seen_ids.insert(id.clone()) {
            continue;
        }
        let mut payload = BTreeMap::new();
        if !symbol.bases.is_empty() {
            payload.insert("bases".to_owned(), symbol.bases.join(","));
        }
        if !symbol.implements.is_empty() {
            payload.insert("implements".to_owned(), symbol.implements.join(","));
        }
        if let Some(target) = &symbol.import_target {
            payload.insert("target".to_owned(), target.clone());
        }
        let location = line_span(symbol.start_line, symbol.end_line);
        nodes.push(KnowledgeNode {
            id: id.clone(),
            node_type: kind,
            name: symbol.name.clone(),
            scope: module.module.clone(),
            summary: symbol.summary.clone(),
            status: "active".to_owned(),
            provenance: observed(
                relative,
                &location,
                extractor,
                extractor_version,
                "tree-sitter",
            ),
            confidence_millis: CONFIDENCE_STRUCTURAL,
            version: NODE_VERSION,
            fingerprint: node_fingerprint(
                kind,
                &symbol.name,
                &module.module,
                ProvenanceKind::Observed,
                &symbol.body,
            ),
            source_refs: vec![relative.to_owned()],
            retrieval_metadata: BTreeMap::new(),
            payload,
            source_id: relative.to_owned(),
            parent: symbol.parent.clone(),
        });
        if !symbol.body.trim().is_empty() {
            let frag_kind = if kind == NodeKind::Import {
                "import"
            } else {
                "code"
            };
            fragments.push(fragment(
                &id,
                0,
                relative,
                &location,
                &symbol.body,
                frag_kind,
            ));
        }
        edges.push(structural_edge(
            &module_id,
            RelationType::Contains,
            &id,
            relative,
            extractor,
            extractor_version,
            &location,
        ));
        if let Some(parent) = &symbol.parent
            && let Some(parent_node) = nodes.iter().find(|node| {
                node.name == *parent
                    && matches!(node.node_type, NodeKind::Class | NodeKind::Interface)
            })
        {
            let parent_id = parent_node.id.clone();
            edges.push(structural_edge(
                &parent_id,
                RelationType::Contains,
                &id,
                relative,
                extractor,
                extractor_version,
                &location,
            ));
        }
    }

    link_local_types(&nodes, relative, extractor, extractor_version, &mut edges);

    Draft {
        source_id: relative.to_owned(),
        language: extractor.to_owned(),
        fingerprint: fingerprint.to_owned(),
        byte_len,
        nodes,
        edges,
        fragments,
        raw: symbols,
        module: Some(module),
    }
}

/// Builds a draft from a markdown file. Mention edges are added by [`mentions`].
pub fn draft_doc(relative: &str, fingerprint: &str, byte_len: i64, doc: &DocExtract) -> Draft {
    let location = doc
        .sections
        .first()
        .map(|section| line_span(section.start_line, section.end_line))
        .unwrap_or_else(|| "L1-L1".to_owned());
    let body = doc
        .sections
        .iter()
        .map(|section| section.content.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let node = KnowledgeNode {
        id: doc.id.clone(),
        node_type: doc.kind,
        name: doc.name.clone(),
        scope: relative.to_owned(),
        summary: doc.summary.clone(),
        status: "active".to_owned(),
        provenance: Provenance {
            kind: ProvenanceKind::Declared,
            source: relative.to_owned(),
            location: location.clone(),
            extractor: "markdown".to_owned(),
            extractor_version: "1".to_owned(),
            method: "markdown-headings".to_owned(),
        },
        confidence_millis: CONFIDENCE_DECLARED,
        version: NODE_VERSION,
        fingerprint: node_fingerprint(
            doc.kind,
            &doc.name,
            relative,
            ProvenanceKind::Declared,
            &body,
        ),
        source_refs: vec![relative.to_owned()],
        retrieval_metadata: BTreeMap::new(),
        payload: BTreeMap::new(),
        source_id: relative.to_owned(),
        parent: None,
    };
    let mut fragments = Vec::new();
    for (index, section) in doc.sections.iter().enumerate() {
        fragments.push(fragment(
            &doc.id,
            u32::try_from(index).unwrap_or(u32::MAX),
            relative,
            &line_span(section.start_line, section.end_line),
            &section.content,
            "section",
        ));
    }
    Draft {
        source_id: relative.to_owned(),
        language: "markdown".to_owned(),
        fingerprint: fingerprint.to_owned(),
        byte_len,
        nodes: vec![node],
        edges: Vec::new(),
        fragments,
        raw: Vec::new(),
        module: None,
    }
}

/// A file the scanner tracks but does not extract (manifests, SQL, CI, …).
pub fn draft_blob(relative: &str, fingerprint: &str, byte_len: i64) -> Draft {
    Draft {
        source_id: relative.to_owned(),
        language: String::new(),
        fingerprint: fingerprint.to_owned(),
        byte_len,
        nodes: Vec::new(),
        edges: Vec::new(),
        fragments: Vec::new(),
        raw: Vec::new(),
        module: None,
    }
}

/// Resolves imports, calls, and tests using `stored` symbols plus the drafts themselves.
pub fn link(drafts: &mut [Draft], stored: &[StoredSymbol]) {
    let index = Index::build(drafts, stored);
    for draft in drafts.iter_mut() {
        let Some(module) = draft.module.clone() else {
            continue;
        };
        let extractor = draft.language.clone();
        let module_id = id::container_id(&module, "module");
        let mut extra = Vec::new();
        for symbol in &draft.raw {
            if symbol.kind == NodeKind::Import
                && let Some(target) = &symbol.import_target
                && let Some(to) = index.modules.get(target)
            {
                extra.push(resolved_edge(
                    &module_id,
                    RelationType::Imports,
                    to,
                    &draft.source_id,
                    &extractor,
                    &line_span(symbol.start_line, symbol.end_line),
                    "tree-sitter+import",
                ));
                continue;
            }
            if symbol.kind == NodeKind::Import {
                continue;
            }
            let kind = reclassify(symbol, id::is_test_path(&draft.source_id));
            let qualifier = match (&symbol.parent, kind) {
                (Some(parent), NodeKind::Method | NodeKind::Test) => {
                    format!("{parent}.{}", symbol.name)
                }
                _ => symbol.name.clone(),
            };
            let caller = id::symbol_id(&module, kind.as_str(), &qualifier);
            for call in &symbol.calls {
                if let Some(edge) = resolve_call(
                    &index,
                    &caller,
                    kind,
                    symbol.parent.as_deref(),
                    call,
                    &draft.source_id,
                    &extractor,
                ) {
                    extra.push(edge.0);
                    if let Some(class_edge) = edge.1 {
                        extra.push(class_edge);
                    }
                }
            }
        }
        for edge in extra {
            if !draft.edges.iter().any(|existing| existing.id == edge.id) {
                draft.edges.push(edge);
            }
        }
    }
}

/// Inferred `REFERENCES` edges from documents to uniquely named types and functions.
///
/// The kind is always [`ProvenanceKind::Inferred`].
pub fn mentions(docs: &[Draft], symbols: &[StoredSymbol]) -> Vec<KnowledgeEdge> {
    let mut by_name: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for symbol in symbols {
        if matches!(
            symbol.kind,
            NodeKind::Class | NodeKind::Interface | NodeKind::Function
        ) && symbol.name.len() >= 4
        {
            by_name
                .entry(symbol.name.as_str())
                .or_default()
                .push(symbol.id.as_str());
        }
    }
    let unique: BTreeMap<&str, &str> = by_name
        .into_iter()
        .filter_map(|(name, ids)| (ids.len() == 1).then_some((name, ids[0])))
        .collect();
    let mut edges = Vec::new();
    for doc in docs {
        let Some(node) = doc.nodes.first() else {
            continue;
        };
        if !matches!(node.node_type, NodeKind::Document | NodeKind::Decision) {
            continue;
        }
        let text = doc
            .fragments
            .iter()
            .map(|frag| frag.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        for (name, target) in &unique {
            if !docs::contains_word(&text, name) {
                continue;
            }
            let line = docs::word_line(&text, name).unwrap_or(1);
            edges.push(KnowledgeEdge {
                id: edge_id(&node.id, RelationType::References, target),
                from_id: node.id.clone(),
                relation: RelationType::References,
                to_id: (*target).to_owned(),
                confidence_millis: CONFIDENCE_INFERRED,
                provenance: Provenance {
                    kind: ProvenanceKind::Inferred,
                    source: doc.source_id.clone(),
                    location: line_span(line, line),
                    extractor: "markdown".to_owned(),
                    extractor_version: "1".to_owned(),
                    method: "name-mention".to_owned(),
                },
                metadata: BTreeMap::new(),
                source_id: doc.source_id.clone(),
            });
        }
    }
    edges.sort_by(|a, b| a.id.cmp(&b.id));
    edges
}

/// Rejects two files that claim one logical id.
pub fn check_unique(nodes: &[KnowledgeNode]) -> Result<(), KnowledgeError> {
    let mut seen: BTreeMap<&str, &str> = BTreeMap::new();
    for node in nodes {
        if let Some(first) = seen.insert(node.id.as_str(), node.source_id.as_str())
            && first != node.source_id
        {
            return Err(KnowledgeError::DuplicateId {
                id: node.id.clone(),
                first: first.to_owned(),
                second: node.source_id.clone(),
            });
        }
    }
    Ok(())
}

fn reclassify(symbol: &RawSymbol, test_file: bool) -> NodeKind {
    if !test_file {
        return symbol.kind;
    }
    match symbol.kind {
        NodeKind::Function => NodeKind::Test,
        NodeKind::Method if symbol.name.starts_with("test_") => NodeKind::Test,
        other => other,
    }
}

fn link_local_types(
    nodes: &[KnowledgeNode],
    relative: &str,
    extractor: &str,
    version: &str,
    edges: &mut Vec<KnowledgeEdge>,
) {
    for node in nodes {
        let bases = node.payload.get("bases").map(|s| s.as_str()).unwrap_or("");
        let implements = node
            .payload
            .get("implements")
            .map(|s| s.as_str())
            .unwrap_or("");
        for base in split_names(bases) {
            if let Some(target) = nodes.iter().find(|other| {
                other.name == base
                    && matches!(other.node_type, NodeKind::Class | NodeKind::Interface)
            }) {
                edges.push(structural_edge(
                    &node.id,
                    RelationType::Extends,
                    &target.id,
                    relative,
                    extractor,
                    version,
                    &node.provenance.location,
                ));
            }
        }
        for iface in split_names(implements) {
            if let Some(target) = nodes
                .iter()
                .find(|other| other.name == iface && other.node_type == NodeKind::Interface)
            {
                edges.push(structural_edge(
                    &node.id,
                    RelationType::Implements,
                    &target.id,
                    relative,
                    extractor,
                    version,
                    &node.provenance.location,
                ));
            }
        }
    }
}

fn split_names(value: &str) -> Vec<&str> {
    value
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect()
}

struct Index {
    classes: BTreeMap<String, Vec<String>>,
    interfaces: BTreeMap<String, Vec<String>>,
    methods: BTreeMap<(String, String), Vec<String>>,
    functions: BTreeMap<String, Vec<String>>,
    modules: BTreeMap<String, String>,
}

impl Index {
    fn build(drafts: &[Draft], stored: &[StoredSymbol]) -> Self {
        let mut index = Self {
            classes: BTreeMap::new(),
            interfaces: BTreeMap::new(),
            methods: BTreeMap::new(),
            functions: BTreeMap::new(),
            modules: BTreeMap::new(),
        };
        for symbol in stored {
            index.insert(
                symbol.kind,
                &symbol.name,
                symbol.parent.as_deref(),
                &symbol.scope,
                &symbol.id,
            );
        }
        for draft in drafts {
            for node in &draft.nodes {
                index.insert(
                    node.node_type,
                    &node.name,
                    node.parent.as_deref(),
                    &node.scope,
                    &node.id,
                );
            }
        }
        index
    }

    fn insert(&mut self, kind: NodeKind, name: &str, parent: Option<&str>, scope: &str, id: &str) {
        match kind {
            NodeKind::Class => self
                .classes
                .entry(name.to_owned())
                .or_default()
                .push(id.to_owned()),
            NodeKind::Interface => self
                .interfaces
                .entry(name.to_owned())
                .or_default()
                .push(id.to_owned()),
            NodeKind::Method => {
                if let Some(parent) = parent {
                    self.methods
                        .entry((parent.to_owned(), name.to_owned()))
                        .or_default()
                        .push(id.to_owned());
                }
            }
            NodeKind::Function => self
                .functions
                .entry(name.to_owned())
                .or_default()
                .push(id.to_owned()),
            NodeKind::Module => {
                self.modules.insert(scope.to_owned(), id.to_owned());
            }
            _ => {}
        }
    }
}

fn resolve_call(
    index: &Index,
    caller: &str,
    caller_kind: NodeKind,
    caller_parent: Option<&str>,
    call: &crate::extract::RawCall,
    source: &str,
    extractor: &str,
) -> Option<(KnowledgeEdge, Option<KnowledgeEdge>)> {
    let location = line_span(call.line, call.line);
    let relation = if caller_kind == NodeKind::Test {
        RelationType::Tests
    } else {
        RelationType::Calls
    };
    if let Some(receiver_type) = &call.receiver_type {
        let method = index
            .methods
            .get(&(receiver_type.clone(), call.callee.clone()))
            .and_then(|ids| only(ids))
            .cloned();
        let class = unique_type(index, receiver_type);
        let target = method.clone().or_else(|| class.clone())?;
        let edge = resolved_edge(
            caller,
            relation,
            &target,
            source,
            extractor,
            &location,
            "tree-sitter+annotation",
        );
        let class_edge = if caller_kind == NodeKind::Test {
            class.filter(|id| id != &target).map(|id| {
                resolved_edge(
                    caller,
                    RelationType::Tests,
                    &id,
                    source,
                    extractor,
                    &location,
                    "tree-sitter+annotation",
                )
            })
        } else {
            caller_parent.and_then(|parent| {
                let from = unique_type(index, parent)?;
                let to = class?;
                Some(resolved_edge(
                    &from,
                    RelationType::Calls,
                    &to,
                    source,
                    extractor,
                    &location,
                    "tree-sitter+annotation",
                ))
            })
        };
        return Some((edge, class_edge));
    }
    if call.receiver.is_some() {
        return None;
    }
    let target = unique_type(index, &call.callee).or_else(|| {
        index
            .functions
            .get(&call.callee)
            .and_then(|ids| only(ids))
            .cloned()
    })?;
    Some((
        resolved_edge(
            caller,
            relation,
            &target,
            source,
            extractor,
            &location,
            "tree-sitter+name",
        ),
        None,
    ))
}

fn only(ids: &[String]) -> Option<&String> {
    (ids.len() == 1).then(|| &ids[0])
}

fn unique_type(index: &Index, name: &str) -> Option<String> {
    let classes = index.classes.get(name).map(Vec::as_slice).unwrap_or(&[]);
    let interfaces = index.interfaces.get(name).map(Vec::as_slice).unwrap_or(&[]);
    let mut ids: Vec<&str> = classes
        .iter()
        .map(String::as_str)
        .chain(interfaces.iter().map(String::as_str))
        .collect();
    ids.sort_unstable();
    ids.dedup();
    (ids.len() == 1).then(|| ids[0].to_owned())
}

fn container_node(
    id: String,
    kind: NodeKind,
    name: &str,
    scope: &str,
    relative: &str,
    extractor: &str,
    version: &str,
) -> KnowledgeNode {
    KnowledgeNode {
        id,
        node_type: kind,
        name: name.to_owned(),
        scope: scope.to_owned(),
        summary: name.to_owned(),
        status: "active".to_owned(),
        provenance: observed(relative, "L1-L1", extractor, version, "tree-sitter"),
        confidence_millis: CONFIDENCE_STRUCTURAL,
        version: NODE_VERSION,
        fingerprint: node_fingerprint(kind, name, scope, ProvenanceKind::Observed, ""),
        source_refs: vec![relative.to_owned()],
        retrieval_metadata: BTreeMap::new(),
        payload: BTreeMap::new(),
        source_id: relative.to_owned(),
        parent: None,
    }
}

fn observed(
    source: &str,
    location: &str,
    extractor: &str,
    version: &str,
    method: &str,
) -> Provenance {
    Provenance {
        kind: ProvenanceKind::Observed,
        source: source.to_owned(),
        location: location.to_owned(),
        extractor: extractor.to_owned(),
        extractor_version: version.to_owned(),
        method: method.to_owned(),
    }
}

fn structural_edge(
    from: &str,
    relation: RelationType,
    to: &str,
    source: &str,
    extractor: &str,
    version: &str,
    location: &str,
) -> KnowledgeEdge {
    KnowledgeEdge {
        id: edge_id(from, relation, to),
        from_id: from.to_owned(),
        relation,
        to_id: to.to_owned(),
        confidence_millis: CONFIDENCE_STRUCTURAL,
        provenance: observed(source, location, extractor, version, "tree-sitter"),
        metadata: BTreeMap::new(),
        source_id: source.to_owned(),
    }
}

fn resolved_edge(
    from: &str,
    relation: RelationType,
    to: &str,
    source: &str,
    extractor: &str,
    location: &str,
    method: &str,
) -> KnowledgeEdge {
    KnowledgeEdge {
        id: edge_id(from, relation, to),
        from_id: from.to_owned(),
        relation,
        to_id: to.to_owned(),
        confidence_millis: CONFIDENCE_RESOLVED,
        provenance: observed(source, location, extractor, "1", method),
        metadata: BTreeMap::new(),
        source_id: source.to_owned(),
    }
}

fn file_name(relative: &str) -> &str {
    relative.rsplit('/').next().unwrap_or(relative)
}
