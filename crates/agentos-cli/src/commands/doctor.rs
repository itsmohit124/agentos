//! `agentos doctor`: report on the installation and project state.

use std::fmt::Write as _;
use std::process::ExitCode;

use agentos_config::OutputFormat;
use agentos_core::db::{self, DbError};
use agentos_core::fs::FileSystem;
use agentos_core::git::Git;
use serde::Serialize;

use crate::app::App;
use crate::output;

/// Exit code when at least one critical check fails.
const EXIT_CHECK_FAILED: u8 = 3;

/// Outcome of one check. Only `Fail` is critical.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
enum Status {
    Pass,
    Info,
    Warn,
    Fail,
}

impl Status {
    fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Fail => "fail",
        }
    }
}

#[derive(Debug, Serialize)]
struct Check {
    name: &'static str,
    status: Status,
    message: String,
}

impl Check {
    fn new(name: &'static str, status: Status, message: impl Into<String>) -> Self {
        Self {
            name,
            status,
            message: message.into(),
        }
    }
}

#[derive(Debug, Serialize)]
struct Report {
    ok: bool,
    version: &'static str,
    root: String,
    checks: Vec<Check>,
}

/// Runs `agentos doctor`.
pub fn run(app: &App, format: OutputFormat) -> anyhow::Result<ExitCode> {
    let checks = vec![
        check_root(app),
        check_git(app),
        check_state_dir(app),
        check_config(app),
        check_database(app),
        check_writable(app),
    ];
    let report = Report {
        ok: checks.iter().all(|c| c.status != Status::Fail),
        version: env!("CARGO_PKG_VERSION"),
        root: app.root.path.display().to_string(),
        checks,
    };
    match format {
        OutputFormat::Json => output::json(&report)?,
        OutputFormat::Text => output::text(&render_text(&report))?,
    }
    Ok(if report.ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(EXIT_CHECK_FAILED)
    })
}

fn check_root(app: &App) -> Check {
    Check::new(
        "root",
        Status::Pass,
        format!("{} (source: {})", app.root.path.display(), app.root.source),
    )
}

fn check_git(app: &App) -> Check {
    if !app.git.is_available() {
        return Check::new("git", Status::Info, "git is not available");
    }
    match app.git.repo_root(&app.root.path) {
        Ok(Some(repo)) => Check::new(
            "git",
            Status::Pass,
            format!("git repository at {}", repo.display()),
        ),
        Ok(None) => Check::new("git", Status::Info, "not a git repository"),
        Err(err) => Check::new("git", Status::Warn, err.to_string()),
    }
}

fn check_state_dir(app: &App) -> Check {
    let dir = app.layout.dir();
    if app.fs.is_dir(&dir) {
        Check::new("state_dir", Status::Pass, dir.display().to_string())
    } else if app.fs.exists(&dir) {
        Check::new(
            "state_dir",
            Status::Fail,
            format!("{} exists but is not a directory", dir.display()),
        )
    } else {
        Check::new(
            "state_dir",
            Status::Fail,
            format!("{} is missing (run `agentos init`)", dir.display()),
        )
    }
}

fn check_config(app: &App) -> Check {
    match &app.config {
        Ok(resolved) if resolved.file_loaded => Check::new(
            "config",
            Status::Pass,
            format!("valid ({})", resolved.file.display()),
        ),
        Ok(_) => Check::new(
            "config",
            Status::Pass,
            "valid (no config file, using defaults)",
        ),
        Err(err) => Check::new("config", Status::Fail, err.to_string()),
    }
}

fn check_database(app: &App) -> Check {
    let path = app.layout.db_file();
    match db::inspect(&path) {
        Ok(status) if status.pending() == 0 => Check::new(
            "database",
            Status::Pass,
            format!("opens, schema version {}", status.current_version),
        ),
        Ok(status) => Check::new(
            "database",
            Status::Warn,
            format!(
                "schema version {} with {} pending migration(s) (run `agentos init`)",
                status.current_version,
                status.pending()
            ),
        ),
        Err(DbError::NotFound(_)) => Check::new(
            "database",
            Status::Fail,
            format!("{} is missing (run `agentos init`)", path.display()),
        ),
        Err(err) => Check::new(
            "database",
            Status::Fail,
            format!("{}: {err}", path.display()),
        ),
    }
}

fn check_writable(app: &App) -> Check {
    let mut dirs = vec![app.root.path.clone()];
    let state_dir = app.layout.dir();
    if app.fs.is_dir(&state_dir) {
        dirs.push(state_dir);
    }
    for dir in &dirs {
        if let Err(err) = app.fs.check_writable(dir) {
            return Check::new("writable", Status::Fail, err.to_string());
        }
    }
    let names: Vec<String> = dirs.iter().map(|d| d.display().to_string()).collect();
    Check::new(
        "writable",
        Status::Pass,
        format!("writable: {}", names.join(", ")),
    )
}

fn render_text(report: &Report) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "AgentOS doctor (agentos {})", report.version);
    for c in &report.checks {
        let _ = writeln!(
            out,
            "  [{}] {:<10} {}",
            c.status.as_str(),
            c.name,
            c.message
        );
    }
    let failed = report
        .checks
        .iter()
        .filter(|c| c.status == Status::Fail)
        .count();
    if failed == 0 {
        let _ = writeln!(out, "Result: OK");
    } else {
        let _ = writeln!(out, "Result: FAILED ({failed} critical check(s) failed)");
    }
    out
}
