//! `agentos cleanup`: remove derived project state and managed adapter blocks.
//!
//! The command does not restore invocation seals and does not delete skill files.

use std::fmt::Write as _;
use std::fs;
use std::process::ExitCode;

use agentos_adapter_claude::ClaudeAdapter;
use agentos_adapter_codex::CodexAdapter;
use agentos_adapter_cursor::CursorAdapter;
use agentos_adapter_generic::{AgentAdapter, GenericAdapter};
use agentos_config::OutputFormat;
use serde::Serialize;

use crate::app::App;
use crate::cli::CleanupArgs;
use crate::output;

const EXIT_USAGE: u8 = 2;
const EXIT_CHECK_FAILED: u8 = 3;
const ADAPTERS: &[&str] = &["claude", "codex", "cursor", "generic"];

#[derive(Serialize)]
struct Report {
    confirmed: bool,
    removed_state: bool,
    kept: Vec<String>,
    adapters: Vec<String>,
}

/// Runs `agentos cleanup`.
pub fn run(app: &App, args: &CleanupArgs, format: OutputFormat) -> anyhow::Result<ExitCode> {
    let root = &app.root.path;
    let state = root.join(".agentos");
    let kept = vec![
        ".agents/skills/".to_owned(),
        "agents/openai.yaml".to_owned(),
    ];
    if !args.yes {
        let mut text = String::from("cleanup needs confirmation\n");
        text.push_str("  would remove .agentos/ after uninstalling managed adapter blocks\n");
        text.push_str("  would keep .agents/skills/ and agents/openai.yaml\n");
        text.push_str("Re-run with --yes to remove them.\n");
        let report = Report {
            confirmed: false,
            removed_state: false,
            kept,
            adapters: ADAPTERS.iter().map(|name| (*name).to_owned()).collect(),
        };
        emit(format, &report, &text)?;
        return Ok(ExitCode::from(EXIT_USAGE));
    }

    let mut failed = false;
    let mut text = String::from("cleanup\n");
    for name in ADAPTERS {
        let adapter = open(name);
        let report = adapter.uninstall(root)?;
        if !report.is_ok() {
            failed = true;
        }
        let _ = writeln!(
            text,
            "  uninstall {name}: {}",
            if report.is_ok() { "ok" } else { "failed" }
        );
    }
    if failed {
        text.push_str("  .agentos/ was left in place\n");
        let report = Report {
            confirmed: true,
            removed_state: false,
            kept,
            adapters: ADAPTERS.iter().map(|name| (*name).to_owned()).collect(),
        };
        emit(format, &report, &text)?;
        return Ok(ExitCode::from(EXIT_CHECK_FAILED));
    }

    let removed_state = if state.is_dir() {
        fs::remove_dir_all(&state)?;
        true
    } else {
        false
    };
    text.push_str(if removed_state {
        "  removed .agentos/\n"
    } else {
        "  .agentos/ was already absent\n"
    });
    text.push_str("  kept .agents/skills/ and agents/openai.yaml\n");
    let report = Report {
        confirmed: true,
        removed_state,
        kept,
        adapters: ADAPTERS.iter().map(|name| (*name).to_owned()).collect(),
    };
    emit(format, &report, &text)?;
    Ok(ExitCode::SUCCESS)
}

fn open(name: &str) -> Box<dyn AgentAdapter> {
    match name {
        "claude" => Box::new(ClaudeAdapter),
        "codex" => Box::new(CodexAdapter),
        "cursor" => Box::new(CursorAdapter),
        _ => Box::new(GenericAdapter),
    }
}

fn emit<T: Serialize>(format: OutputFormat, value: &T, text: &str) -> anyhow::Result<()> {
    match format {
        OutputFormat::Json => output::json(value),
        OutputFormat::Text => output::text(text),
    }
}
