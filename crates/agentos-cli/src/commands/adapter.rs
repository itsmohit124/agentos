//! `agentos adapter`: list, install, uninstall, and doctor.

use std::fmt::Write as _;
use std::process::ExitCode;

use agentos_adapter_claude::ClaudeAdapter;
use agentos_adapter_codex::CodexAdapter;
use agentos_adapter_cursor::CursorAdapter;
use agentos_adapter_generic::{AgentAdapter, GenericAdapter};
use agentos_config::OutputFormat;
use serde::Serialize;

use crate::app::App;
use crate::cli::AdapterCommand;
use crate::output;

/// Exit code when doctor finds a failed check.
const EXIT_CHECK_FAILED: u8 = 3;

/// Runs `agentos adapter`.
pub fn run(app: &App, command: &AdapterCommand, format: OutputFormat) -> anyhow::Result<ExitCode> {
    match command {
        AdapterCommand::List => list(format),
        AdapterCommand::Install { adapter } => {
            let adapter = open(adapter)?;
            let report = adapter.install(&app.root.path)?;
            emit(
                format,
                &report,
                &render_files(&report.adapter, "install", &report.files),
            )?;
            Ok(ExitCode::SUCCESS)
        }
        AdapterCommand::Uninstall { adapter } => {
            let adapter = open(adapter)?;
            let report = adapter.uninstall(&app.root.path)?;
            emit(
                format,
                &report,
                &render_files(&report.adapter, "uninstall", &report.files),
            )?;
            if report.is_ok() {
                Ok(ExitCode::SUCCESS)
            } else {
                Ok(ExitCode::from(EXIT_CHECK_FAILED))
            }
        }
        AdapterCommand::Doctor { adapter } => {
            let adapter = open(adapter)?;
            let report = adapter.doctor(&app.root.path)?;
            emit(format, &report, &render_doctor(&report))?;
            if report.ok {
                Ok(ExitCode::SUCCESS)
            } else {
                Ok(ExitCode::from(EXIT_CHECK_FAILED))
            }
        }
    }
}

fn list(format: OutputFormat) -> anyhow::Result<ExitCode> {
    let adapters = all();
    let caps: Vec<_> = adapters
        .iter()
        .map(|adapter| adapter.capabilities())
        .collect();
    match format {
        OutputFormat::Json => output::json(&caps)?,
        OutputFormat::Text => {
            let mut text = String::new();
            for cap in &caps {
                let _ = writeln!(
                    text,
                    "{:<8} install={} skills={} context={} workflow={} feedback={} external={}",
                    cap.id,
                    support(cap.install),
                    support(cap.skills),
                    support(cap.context),
                    support(cap.workflow),
                    support(cap.feedback),
                    support(cap.external_agent),
                );
            }
            output::text(&text)?;
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn open(name: &str) -> anyhow::Result<Box<dyn AgentAdapter>> {
    all()
        .into_iter()
        .find(|adapter| adapter.id() == name)
        .ok_or_else(|| {
            anyhow::anyhow!("unknown adapter {name:?} (known: claude, codex, cursor, generic)")
        })
}

fn all() -> Vec<Box<dyn AgentAdapter>> {
    vec![
        Box::new(ClaudeAdapter),
        Box::new(CodexAdapter),
        Box::new(CursorAdapter),
        Box::new(GenericAdapter),
    ]
}

fn support(value: agentos_adapter_generic::Support) -> &'static str {
    match value {
        agentos_adapter_generic::Support::Supported => "supported",
        agentos_adapter_generic::Support::Unsupported => "unsupported",
        agentos_adapter_generic::Support::Unavailable => "unavailable",
        agentos_adapter_generic::Support::Optional => "optional",
    }
}

fn emit<T: Serialize>(format: OutputFormat, value: &T, text: &str) -> anyhow::Result<()> {
    match format {
        OutputFormat::Json => output::json(value),
        OutputFormat::Text => output::text(text),
    }
}

fn render_files(
    adapter: &str,
    operation: &str,
    files: &[agentos_adapter_generic::FileAction],
) -> String {
    let mut text = format!("adapter {adapter} {operation}\n");
    if files.is_empty() {
        text.push_str("  (no files)\n");
        return text;
    }
    for file in files {
        let detail = if file.detail.is_empty() {
            String::new()
        } else {
            format!(" ({})", file.detail)
        };
        let _ = writeln!(
            text,
            "  {:<10} {}{detail}",
            format!("{:?}", file.action).to_lowercase(),
            file.path
        );
    }
    text
}

fn render_doctor(report: &agentos_adapter_generic::DoctorReport) -> String {
    let mut text = format!(
        "adapter {} doctor: {}\n",
        report.adapter,
        if report.ok { "ok" } else { "failed" }
    );
    for check in &report.checks {
        let _ = writeln!(
            text,
            "  {:<6} {:<18} {}",
            check.status, check.name, check.detail
        );
    }
    text
}
