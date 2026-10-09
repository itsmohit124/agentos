//! `agentos domain`: bounded contexts, terminology, proposals, and confirmation.

use std::fmt::Write as _;
use std::process::ExitCode;

use agentos_config::OutputFormat;
use agentos_knowledge::{
    DomainReport, ProposeReport, StatusChange, TerminologyView, ValidationReport,
};
use anyhow::Context;

use crate::app::App;
use crate::cli::DomainCommand;
use crate::output;

/// Exit code when `domain validate` finds drift or a lifecycle break.
const EXIT_FAILED: u8 = 3;

/// Runs `agentos domain`.
pub fn run(app: &App, command: &DomainCommand, format: OutputFormat) -> anyhow::Result<ExitCode> {
    let root = &app.root.path;
    match command {
        DomainCommand::Scan => {
            let report = agentos_knowledge::domain_scan(root)
                .with_context(|| format!("cannot scan domain knowledge in {}", root.display()))?;
            emit(format, &report, &render_scan(&report))
        }
        DomainCommand::Contexts => {
            let nodes = agentos_knowledge::contexts(root)?;
            let text = if nodes.is_empty() {
                "no bounded contexts\n".to_owned()
            } else {
                nodes
                    .iter()
                    .map(|node| format!("{}  {}  {}", node.id, node.status, node.confidence_millis))
                    .collect::<Vec<_>>()
                    .join("\n")
                    + "\n"
            };
            emit(format, &nodes, &text)
        }
        DomainCommand::Terminology => {
            let view = agentos_knowledge::terminology(root)?;
            emit(format, &view, &render_terms(&view))
        }
        DomainCommand::Map => {
            let edges = agentos_knowledge::context_map(root)?;
            let text = if edges.is_empty() {
                "no context-map edges\n".to_owned()
            } else {
                edges
                    .iter()
                    .map(|edge| {
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
                        format!(
                            "{}  {}  {}  downstream {downstream} upstream {upstream}",
                            edge.from_id,
                            edge.relation.as_str(),
                            edge.to_id
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
                    + "\n"
            };
            emit(format, &edges, &text)
        }
        DomainCommand::Propose => {
            let report = agentos_knowledge::propose(root)?;
            emit(format, &report, &render_propose(&report))
        }
        DomainCommand::Confirm { id, by, reason } => {
            let change = agentos_knowledge::confirm(
                root,
                id,
                by.as_deref().unwrap_or("operator"),
                reason.as_deref().unwrap_or("operator confirmation"),
            )?;
            emit(format, &change, &render_change("Confirmed", &change))
        }
        DomainCommand::Reject { id, by, reason } => {
            let change = agentos_knowledge::reject(
                root,
                id,
                by.as_deref().unwrap_or("operator"),
                reason.as_deref().unwrap_or("operator rejection"),
            )?;
            emit(format, &change, &render_change("Rejected", &change))
        }
        DomainCommand::Validate => {
            let report = agentos_knowledge::validate(root)?;
            let code = if report.ok {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(EXIT_FAILED)
            };
            emit_code(format, &report, &render_validate(&report), code)
        }
    }
}

fn emit<T: serde::Serialize>(
    format: OutputFormat,
    value: &T,
    text: &str,
) -> anyhow::Result<ExitCode> {
    emit_code(format, value, text, ExitCode::SUCCESS)
}

fn emit_code<T: serde::Serialize>(
    format: OutputFormat,
    value: &T,
    text: &str,
    code: ExitCode,
) -> anyhow::Result<ExitCode> {
    match format {
        OutputFormat::Json => output::json(value)?,
        OutputFormat::Text => output::text(text)?,
    }
    Ok(code)
}

fn render_scan(report: &DomainReport) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "Domain scan: {} nodes, {} edges, {} findings",
        report.nodes, report.edges, report.findings
    );
    let _ = writeln!(out, "rows written {}", report.rows_written);
    for (status, count) in &report.by_status {
        let _ = writeln!(out, "{status} {count}");
    }
    out
}

fn render_propose(report: &ProposeReport) -> String {
    format!(
        "Promoted {} node(s) to the next proposed step\nproposer {} added {}\n",
        report.promoted, report.proposer, report.added
    )
}

fn render_change(verb: &str, change: &StatusChange) -> String {
    format!("{verb} {} ({} -> {})\n", change.id, change.from, change.to)
}

fn render_terms(view: &TerminologyView) -> String {
    let mut out = String::new();
    if view.terms.is_empty() {
        let _ = writeln!(out, "no terminology");
    }
    for term in &view.terms {
        let _ = writeln!(out, "{}  {}  {}", term.id, term.name, term.status);
    }
    if view.ambiguities.is_empty() {
        let _ = writeln!(out, "no terminology ambiguity");
    }
    for finding in &view.ambiguities {
        let _ = writeln!(out, "finding {}  {}", finding.kind, finding.summary);
    }
    out
}

fn render_validate(report: &ValidationReport) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "{}", if report.ok { "ok" } else { "failed" });
    for line in &report.drift {
        let _ = writeln!(out, "drift: {line}");
    }
    for line in &report.lifecycle_violations {
        let _ = writeln!(out, "lifecycle: {line}");
    }
    for line in &report.provenance_incomplete {
        let _ = writeln!(out, "provenance: {line}");
    }
    for finding in &report.findings {
        let _ = writeln!(out, "finding {}  {}", finding.kind, finding.summary);
    }
    out
}
