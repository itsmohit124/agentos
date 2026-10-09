//! `agentos knowledge`: query the derived graph.

use std::fmt::Write as _;
use std::process::ExitCode;

use agentos_config::OutputFormat;
use agentos_knowledge::{Inspection, KnowledgeStore, Stats};
use anyhow::Context;

use crate::app::App;
use crate::cli::KnowledgeCommand;
use crate::output;

/// Runs `agentos knowledge`.
pub fn run(
    app: &App,
    command: &KnowledgeCommand,
    format: OutputFormat,
) -> anyhow::Result<ExitCode> {
    if matches!(command, KnowledgeCommand::Rebuild) {
        return crate::commands::scan::rebuild(app, format);
    }
    let store = KnowledgeStore::open(&app.root.path)
        .with_context(|| format!("cannot open knowledge index in {}", app.root.path.display()))?;
    match command {
        KnowledgeCommand::Stats => {
            let stats = store.stats()?;
            emit(format, &stats, &render_stats(&stats))
        }
        KnowledgeCommand::Nodes => {
            let nodes = store.nodes()?;
            let text = nodes
                .iter()
                .map(|node| format!("{}  {}", node.id, node.node_type.as_str()))
                .collect::<Vec<_>>()
                .join("\n");
            let text = if text.is_empty() {
                "no nodes\n".to_owned()
            } else {
                format!("{text}\n")
            };
            emit(format, &nodes, &text)
        }
        KnowledgeCommand::Edges => {
            let edges = store.edges()?;
            let text = edges
                .iter()
                .map(|edge| {
                    format!(
                        "{}  {}  {}",
                        edge.from_id,
                        edge.relation.as_str(),
                        edge.to_id
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            let text = if text.is_empty() {
                "no edges\n".to_owned()
            } else {
                format!("{text}\n")
            };
            emit(format, &edges, &text)
        }
        KnowledgeCommand::Inspect { id } => {
            let inspection = store.inspect(id)?;
            emit(format, &inspection, &render_inspect(&inspection))
        }
        KnowledgeCommand::Rebuild => unreachable!("rebuild is handled above"),
    }
}

fn emit<T: serde::Serialize>(
    format: OutputFormat,
    value: &T,
    text: &str,
) -> anyhow::Result<ExitCode> {
    match format {
        OutputFormat::Json => output::json(value)?,
        OutputFormat::Text => output::text(text)?,
    }
    Ok(ExitCode::SUCCESS)
}

fn render_stats(stats: &Stats) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "files {}  nodes {}  edges {}  fragments {}",
        stats.files, stats.nodes, stats.edges, stats.fragments
    );
    let _ = writeln!(out, "schema_version {}", stats.schema_version);
    let _ = writeln!(out, "nodes by type:");
    for (kind, count) in &stats.nodes_by_type {
        let _ = writeln!(out, "  {kind} {count}");
    }
    let _ = writeln!(out, "edges by type:");
    for (kind, count) in &stats.edges_by_type {
        let _ = writeln!(out, "  {kind} {count}");
    }
    let _ = writeln!(out, "provenance:");
    for (kind, count) in &stats.provenance {
        let _ = writeln!(out, "  {kind} {count}");
    }
    out
}

fn render_inspect(inspection: &Inspection) -> String {
    let node = &inspection.node;
    let mut out = String::new();
    let _ = writeln!(out, "{}", node.id);
    let _ = writeln!(out, "  type: {}", node.node_type.as_str());
    let _ = writeln!(out, "  name: {}", node.name);
    let _ = writeln!(out, "  scope: {}", node.scope);
    let _ = writeln!(out, "  summary: {}", node.summary);
    let _ = writeln!(
        out,
        "  provenance: {} {} {} {}",
        node.provenance.kind.as_str(),
        node.provenance.extractor,
        node.provenance.extractor_version,
        node.provenance.method
    );
    let _ = writeln!(out, "  confidence_millis: {}", node.confidence_millis);
    let _ = writeln!(out, "  source: {}", node.source_id);
    let _ = writeln!(out, "  outgoing:");
    for edge in &inspection.outgoing {
        let _ = writeln!(out, "    {} {}", edge.relation.as_str(), edge.to_id);
    }
    let _ = writeln!(out, "  incoming:");
    for edge in &inspection.incoming {
        let _ = writeln!(out, "    {} {}", edge.relation.as_str(), edge.from_id);
    }
    let _ = writeln!(out, "  fragments:");
    for fragment in &inspection.fragments {
        let _ = writeln!(out, "    {} {}", fragment.id, fragment.location);
    }
    out
}
