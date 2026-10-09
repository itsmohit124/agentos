//! `agentos context`: build a Context Pack from the existing knowledge graph.

use std::process::ExitCode;

use agentos_config::OutputFormat;
use agentos_context::{
    ContextConfig, NullSemantic, SqliteView, build, explain, render_json, stats, to_markdown,
};
use agentos_knowledge::{KnowledgeStore, domain_scan, scan};
use anyhow::Context;

use crate::app::App;
use crate::cli::{ContextArgs, ContextCommand, PackFormat};
use crate::output;

/// Runs `agentos context`.
pub fn run(app: &App, args: &ContextArgs, format: OutputFormat) -> anyhow::Result<ExitCode> {
    let root = &app.root.path;
    scan(root).with_context(|| format!("cannot scan {}", root.display()))?;
    domain_scan(root)
        .with_context(|| format!("cannot scan domain knowledge in {}", root.display()))?;
    let store = KnowledgeStore::open(root)
        .with_context(|| format!("cannot open knowledge index in {}", root.display()))?;
    let view = SqliteView::new(&store);
    let config = ContextConfig {
        budget: args.budget,
        cache_dir: Some(root.join(".agentos/context-cache")),
    };
    let task = task_of(&args.command);
    let pack = build(&view, task, &config, &NullSemantic)?;
    match &args.command {
        ContextCommand::Explain { .. } => match format {
            OutputFormat::Json => output::text(&render_json(&view, task, &config, &NullSemantic)?)?,
            OutputFormat::Text => output::text(&explain(&pack))?,
        },
        ContextCommand::Build {
            format: pack_format,
            ..
        } => {
            if format == OutputFormat::Json || *pack_format == PackFormat::Json {
                output::text(&render_json(&view, task, &config, &NullSemantic)?)?;
            } else {
                output::text(&to_markdown(&pack))?;
            }
        }
        ContextCommand::Stats { .. } => {
            let summary = stats(&pack);
            match format {
                OutputFormat::Json => output::json(&summary)?,
                OutputFormat::Text => output::text(&render_stats(&summary))?,
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn task_of(command: &ContextCommand) -> &str {
    match command {
        ContextCommand::Explain { task }
        | ContextCommand::Build { task, .. }
        | ContextCommand::Stats { task } => task,
    }
}

fn render_stats(summary: &agentos_context::ContextStats) -> String {
    let budget = summary
        .budget
        .map(|limit| limit.to_string())
        .unwrap_or_else(|| "unbounded".to_owned());
    let contexts = if summary.contexts.is_empty() {
        "(none)".to_owned()
    } else {
        summary.contexts.join(" ")
    };
    format!(
        "intent: {}\ncontexts: {contexts}\nselected_nodes: {}\nselected_fragments: {}\ntokens: {}\nbudget: {budget}\nexcluded: {}\n",
        summary.intent,
        summary.selected_nodes,
        summary.selected_fragments,
        summary.tokens,
        summary.excluded
    )
}
