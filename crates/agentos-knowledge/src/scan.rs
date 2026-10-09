//! Scan a repository into the derived knowledge index.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::Serialize;

use crate::assemble::{self, Draft, StoredSymbol};
use crate::docs;
use crate::error::KnowledgeError;
use crate::extract::ExtractorRegistry;
use crate::model::{KnowledgeNode, NodeKind};
use crate::profile::{self, ProfileWrite};
use crate::store::KnowledgeStore;
use crate::walk::{self, SourceFile};

/// What one scan did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScanReport {
    /// Files considered.
    pub files: usize,
    /// Files whose fingerprint changed or that are new.
    pub changed: usize,
    /// Files left untouched.
    pub unchanged: usize,
    /// Files removed from the index.
    pub deleted: usize,
    /// Nodes in the index after the scan.
    pub nodes: usize,
    /// Edges in the index after the scan.
    pub edges: usize,
    /// Fragments in the index after the scan.
    pub fragments: usize,
    /// SQLite rows written. Zero when nothing changed.
    pub rows_written: u64,
    /// What happened to the project profile.
    pub profile: String,
}

/// Scans `root` with the built-in extractors.
pub fn scan(root: &Path) -> Result<ScanReport, KnowledgeError> {
    scan_with(root, &ExtractorRegistry::builtin())
}

/// Scans `root` with `extractors` (built-ins plus anything a caller registered).
pub fn scan_with(
    root: &Path,
    extractors: &ExtractorRegistry,
) -> Result<ScanReport, KnowledgeError> {
    let files = walk::list_sources(root)?;
    let (_, profile_status) = profile::write_profile(root)?;
    let store = KnowledgeStore::open(root)?;
    let existing = store.fingerprints()?;
    let seen: BTreeSet<&str> = files.iter().map(|file| file.relative.as_str()).collect();
    let deleted: Vec<String> = existing
        .keys()
        .filter(|path| !seen.contains(path.as_str()))
        .cloned()
        .collect();
    let mut changed: Vec<&SourceFile> = Vec::new();
    let mut unchanged = 0usize;
    for file in &files {
        match existing.get(&file.relative) {
            Some(fingerprint) if fingerprint == &file.fingerprint => unchanged += 1,
            _ => changed.push(file),
        }
    }
    if changed.is_empty() && deleted.is_empty() {
        let stats = store.stats()?;
        return Ok(report(
            &stats,
            files.len(),
            0,
            unchanged,
            0,
            0,
            profile_status,
        ));
    }

    let mut skip = BTreeMap::new();
    for file in &changed {
        skip.insert(file.relative.clone(), ());
    }
    for path in &deleted {
        skip.insert(path.clone(), ());
    }
    let stored = store.symbols_except(&skip)?;
    let old_names = name_set(&store.mention_targets()?);

    let mut drafts = Vec::new();
    for file in &changed {
        drafts.push(draft_for(file, extractors)?);
    }
    assemble::link(&mut drafts, &stored);
    check_ids(&stored, &drafts)?;

    let new_names = merged_names(&stored, &drafts);
    let names_changed = old_names != new_names;
    let mention_edges = if names_changed {
        let mut docs = document_drafts(&store, &skip)?;
        docs.extend(drafts.iter().filter(|draft| is_doc(draft)).cloned());
        let symbols = mention_symbols(&stored, &drafts);
        Some(assemble::mentions(&docs, &symbols))
    } else {
        let docs: Vec<Draft> = drafts
            .iter()
            .filter(|draft| is_doc(draft))
            .cloned()
            .collect();
        if docs.is_empty() {
            None
        } else {
            let symbols = mention_symbols(&stored, &drafts);
            let edges = assemble::mentions(&docs, &symbols);
            for edge in edges {
                if let Some(draft) = drafts
                    .iter_mut()
                    .find(|item| item.source_id == edge.source_id)
                {
                    draft.edges.push(edge);
                }
            }
            None
        }
    };

    let rows_written = store.apply_changes(&deleted, &drafts, mention_edges.as_deref())?;
    let stats = store.stats()?;
    Ok(report(
        &stats,
        files.len(),
        changed.len(),
        unchanged,
        deleted.len(),
        rows_written,
        profile_status,
    ))
}

/// Deletes the derived graph and scans again.
pub fn rebuild(root: &Path) -> Result<ScanReport, KnowledgeError> {
    rebuild_with(root, &ExtractorRegistry::builtin())
}

/// [`rebuild`] with a caller-supplied extractor registry.
pub fn rebuild_with(
    root: &Path,
    extractors: &ExtractorRegistry,
) -> Result<ScanReport, KnowledgeError> {
    let store = KnowledgeStore::open(root)?;
    store.clear()?;
    scan_with(root, extractors)
}

fn draft_for(file: &SourceFile, extractors: &ExtractorRegistry) -> Result<Draft, KnowledgeError> {
    let text = std::str::from_utf8(&file.bytes).map_err(|err| KnowledgeError::Extract {
        language: "utf-8",
        message: format!("{}: {err}", file.relative),
    })?;
    let byte_len = i64::try_from(file.bytes.len()).unwrap_or(i64::MAX);
    if file.relative.ends_with(".md") {
        let doc = docs::extract(&file.relative, text);
        return Ok(assemble::draft_doc(
            &file.relative,
            &file.fingerprint,
            byte_len,
            &doc,
        ));
    }
    let path = Path::new(&file.relative);
    if let Some(extractor) = extractors.find(path) {
        let symbols = extractor.extract(&file.relative, text)?;
        return Ok(assemble::draft_code(
            &file.relative,
            &file.fingerprint,
            byte_len,
            extractor.name(),
            extractor.version(),
            symbols,
        ));
    }
    Ok(assemble::draft_blob(
        &file.relative,
        &file.fingerprint,
        byte_len,
    ))
}

fn document_drafts(
    store: &KnowledgeStore,
    skip: &BTreeMap<String, ()>,
) -> Result<Vec<Draft>, KnowledgeError> {
    let nodes = store.nodes()?;
    let fragments = store.fragments()?;
    let mut by_source: BTreeMap<String, Draft> = BTreeMap::new();
    for node in nodes {
        if skip.contains_key(&node.source_id) {
            continue;
        }
        if !matches!(node.node_type, NodeKind::Document | NodeKind::Decision) {
            continue;
        }
        by_source.insert(
            node.source_id.clone(),
            Draft {
                source_id: node.source_id.clone(),
                language: "markdown".to_owned(),
                fingerprint: String::new(),
                byte_len: 0,
                nodes: vec![node],
                edges: Vec::new(),
                fragments: Vec::new(),
                raw: Vec::new(),
                module: None,
            },
        );
    }
    for frag in fragments {
        if let Some(draft) = by_source
            .values_mut()
            .find(|draft| draft.nodes.iter().any(|node| node.id == frag.node_id))
        {
            draft.fragments.push(frag);
        }
    }
    Ok(by_source.into_values().collect())
}

fn mention_symbols(stored: &[StoredSymbol], drafts: &[Draft]) -> Vec<StoredSymbol> {
    let mut symbols: Vec<StoredSymbol> = stored
        .iter()
        .filter(|symbol| is_mention_kind(symbol.kind))
        .cloned()
        .collect();
    for draft in drafts {
        for node in &draft.nodes {
            if is_mention_kind(node.node_type) {
                symbols.push(symbol_from_node(node));
            }
        }
    }
    symbols
}

fn is_mention_kind(kind: NodeKind) -> bool {
    matches!(
        kind,
        NodeKind::Class | NodeKind::Interface | NodeKind::Function
    )
}

fn symbol_from_node(node: &KnowledgeNode) -> StoredSymbol {
    StoredSymbol {
        id: node.id.clone(),
        kind: node.node_type,
        name: node.name.clone(),
        parent: node.parent.clone(),
        scope: node.scope.clone(),
    }
}

fn name_set(symbols: &[StoredSymbol]) -> BTreeSet<(String, String)> {
    symbols
        .iter()
        .map(|symbol| (symbol.name.clone(), symbol.id.clone()))
        .collect()
}

fn merged_names(stored: &[StoredSymbol], drafts: &[Draft]) -> BTreeSet<(String, String)> {
    name_set(&mention_symbols(stored, drafts))
}

fn is_doc(draft: &Draft) -> bool {
    draft
        .nodes
        .iter()
        .any(|node| matches!(node.node_type, NodeKind::Document | NodeKind::Decision))
}

fn check_ids(stored: &[StoredSymbol], drafts: &[Draft]) -> Result<(), KnowledgeError> {
    let mut seen: BTreeMap<String, String> = BTreeMap::new();
    for symbol in stored {
        seen.insert(symbol.id.clone(), String::new());
    }
    let mut nodes = Vec::new();
    for draft in drafts {
        for node in &draft.nodes {
            if let Some(first) = seen.get(&node.id) {
                if !first.is_empty() && first != &node.source_id {
                    return Err(KnowledgeError::DuplicateId {
                        id: node.id.clone(),
                        first: first.clone(),
                        second: node.source_id.clone(),
                    });
                }
                if first.is_empty() {
                    return Err(KnowledgeError::DuplicateId {
                        id: node.id.clone(),
                        first: "(indexed)".to_owned(),
                        second: node.source_id.clone(),
                    });
                }
            }
            seen.insert(node.id.clone(), node.source_id.clone());
            nodes.push(node.clone());
        }
    }
    assemble::check_unique(&nodes)
}

fn report(
    stats: &crate::store::Stats,
    files: usize,
    changed: usize,
    unchanged: usize,
    deleted: usize,
    rows_written: u64,
    profile: ProfileWrite,
) -> ScanReport {
    ScanReport {
        files,
        changed,
        unchanged,
        deleted,
        nodes: stats.nodes,
        edges: stats.edges,
        fragments: stats.fragments,
        rows_written,
        profile: profile.as_str().to_owned(),
    }
}
