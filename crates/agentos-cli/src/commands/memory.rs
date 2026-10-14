//! `agentos memory`: list, show, add, confirm, forget, prune, and query.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::ExitCode;

use agentos_config::OutputFormat;
use agentos_context::{
    ContextConfig, MemoryCandidate, NullSemantic, SqliteView, build_with_memory,
};
use agentos_knowledge::{KnowledgeStore, ProvenanceKind, domain_scan, scan};
use agentos_memory::{FingerprintIndex, MemoryLink, MemoryRecord, MemoryStore, NewMemory, utc_now};
use anyhow::Context;

use crate::app::App;
use crate::cli::MemoryCommand;
use crate::output;

/// Runs `agentos memory`.
pub fn run(app: &App, command: &MemoryCommand, format: OutputFormat) -> anyhow::Result<ExitCode> {
    let root = &app.root.path;
    match command {
        MemoryCommand::List => {
            let store = prepared(root)?;
            let records = store.memory.list(&store.index, &utc_now())?;
            emit_list(&records, format)?;
        }
        MemoryCommand::Show { id } => {
            let store = prepared(root)?;
            let record = store.memory.show(id, &store.index, &utc_now())?;
            emit_record(&record, format)?;
        }
        MemoryCommand::Add {
            r#type,
            name,
            content,
            task,
            run,
            outcome,
            links,
            at,
            confirm,
        } => {
            let memory = MemoryStore::open(root)?;
            let record = memory.add(NewMemory {
                memory_type: r#type.as_str().to_owned(),
                name: name.clone(),
                content: content.clone(),
                task: task.clone(),
                run_id: run.clone(),
                outcome: outcome.clone(),
                links: parse_links(links)?,
                recorded_at: stamp(at),
                confirm: *confirm,
                pattern: false,
                intent_kind: String::new(),
                payload: BTreeMap::new(),
                provenance_kind: ProvenanceKind::Declared,
                method: "human-add".to_owned(),
                confidence_millis: 1000,
            })?;
            emit_record(&record, format)?;
        }
        MemoryCommand::Confirm { id, by, at } => {
            let memory = MemoryStore::open(root)?;
            let record = memory.confirm(id, by.as_deref().unwrap_or("operator"), &stamp(at))?;
            emit_record(&record, format)?;
        }
        MemoryCommand::Forget { id, at } => {
            let memory = MemoryStore::open(root)?;
            let record = memory.forget(id, &stamp(at))?;
            emit_record(&record, format)?;
        }
        MemoryCommand::Prune { stale: _, at } => {
            let store = prepared(root)?;
            let removed = store.memory.prune_stale(&store.index, &stamp(at))?;
            match format {
                OutputFormat::Json => output::json(&removed)?,
                OutputFormat::Text => output::text(&render_pruned(&removed))?,
            }
        }
        MemoryCommand::Query { task } => {
            let pack = pack_for(root, task)?;
            let entries: Vec<_> = pack
                .retrieval_trace
                .entries
                .iter()
                .filter(|entry| entry.item_kind == "node" && entry.id.starts_with("memory."))
                .collect();
            match format {
                OutputFormat::Json => output::json(&entries)?,
                OutputFormat::Text => {
                    let mut text = format!("task: {task}\nmemory:\n");
                    if entries.is_empty() {
                        text.push_str("  (none)\n");
                    }
                    for entry in entries {
                        text.push_str(&format!(
                            "  {} {} {}\n",
                            entry.id, entry.outcome, entry.reason
                        ));
                    }
                    output::text(&text)?;
                }
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// Memory candidates for the Context Engine. Empty when the project has no memory.
pub fn candidates(root: &Path, knowledge: &KnowledgeStore) -> anyhow::Result<Vec<MemoryCandidate>> {
    let index = FingerprintIndex::from_store(knowledge)?;
    let memory = MemoryStore::open(root)?;
    Ok(memory.candidates(&index, &utc_now())?)
}

/// Context Pack for `task`, including memory candidates.
pub fn pack_for(root: &Path, task: &str) -> anyhow::Result<agentos_context::ContextPack> {
    scan(root).with_context(|| format!("cannot scan {}", root.display()))?;
    domain_scan(root)
        .with_context(|| format!("cannot scan domain knowledge in {}", root.display()))?;
    let knowledge = KnowledgeStore::open(root)
        .with_context(|| format!("cannot open knowledge index in {}", root.display()))?;
    let view = SqliteView::new(&knowledge);
    let memory = candidates(root, &knowledge)?;
    let config = ContextConfig {
        budget: None,
        cache_dir: Some(root.join(".agentos/context-cache")),
    };
    build_with_memory(&view, task, &config, &NullSemantic, &memory)
        .context("cannot build a context pack")
}

struct Prepared {
    memory: MemoryStore,
    index: FingerprintIndex,
}

fn prepared(root: &Path) -> anyhow::Result<Prepared> {
    scan(root).with_context(|| format!("cannot scan {}", root.display()))?;
    domain_scan(root)
        .with_context(|| format!("cannot scan domain knowledge in {}", root.display()))?;
    let knowledge = KnowledgeStore::open(root)
        .with_context(|| format!("cannot open knowledge index in {}", root.display()))?;
    let index = FingerprintIndex::from_store(&knowledge)?;
    drop(knowledge);
    let memory = MemoryStore::open(root)?;
    Ok(Prepared { memory, index })
}

fn stamp(at: &Option<String>) -> String {
    at.clone().unwrap_or_else(utc_now)
}

fn parse_links(values: &[String]) -> anyhow::Result<Vec<MemoryLink>> {
    let mut links = Vec::new();
    for value in values {
        let mut parts = value.splitn(3, '|');
        let kind = parts.next().unwrap_or("").to_owned();
        let id = parts.next().unwrap_or("").to_owned();
        let fingerprint = parts.next().unwrap_or("").to_owned();
        let link = MemoryLink {
            kind,
            id,
            fingerprint,
        };
        link.validate()?;
        links.push(link);
    }
    Ok(links)
}

fn emit_list(records: &[MemoryRecord], format: OutputFormat) -> anyhow::Result<()> {
    match format {
        OutputFormat::Json => output::json(&records.to_vec()),
        OutputFormat::Text => {
            let mut text = String::new();
            if records.is_empty() {
                text.push_str("(none)\n");
            }
            for record in records {
                text.push_str(&format!(
                    "{} {} {} {} {}\n",
                    record.id, record.memory_type, record.status, record.freshness, record.name
                ));
            }
            output::text(&text)
        }
    }
}

fn emit_record(record: &MemoryRecord, format: OutputFormat) -> anyhow::Result<()> {
    match format {
        OutputFormat::Json => output::json(record),
        OutputFormat::Text => output::text(&render_record(record)),
    }
}

fn render_record(record: &MemoryRecord) -> String {
    let mut text = String::new();
    text.push_str(&format!("id: {}\n", record.id));
    text.push_str(&format!("type: {}\n", record.memory_type));
    text.push_str(&format!("status: {}\n", record.status));
    text.push_str(&format!("freshness: {}\n", record.freshness));
    text.push_str(&format!("name: {}\n", record.name));
    text.push_str(&format!("task: {}\n", record.task));
    text.push_str(&format!("run: {}\n", record.run_id));
    text.push_str(&format!("outcome: {}\n", record.outcome));
    text.push_str(&format!(
        "provenance: {} {} {}\n",
        record.provenance.kind.as_str(),
        record.provenance.source,
        record.provenance.method
    ));
    text.push_str(&format!("canonical: {}\n", record.is_canonical()));
    text.push_str("content:\n");
    text.push_str(&record.content);
    if !record.content.ends_with('\n') {
        text.push('\n');
    }
    text
}

fn render_pruned(ids: &[String]) -> String {
    let mut text = String::from("pruned:\n");
    if ids.is_empty() {
        text.push_str("  (none)\n");
    }
    for id in ids {
        text.push_str(&format!("  {id}\n"));
    }
    text
}
