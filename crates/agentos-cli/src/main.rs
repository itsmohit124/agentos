//! `agentos` command-line interface.
//!
//! Exit codes: `0` success, `1` runtime error, `2` usage error, `3` `doctor` found a critical
//! failure.

mod app;
mod cli;
mod commands;
mod logging;
mod output;

use std::process::ExitCode;

use clap::Parser;

use crate::cli::{Cli, Command};

/// Exit code for a runtime error.
const EXIT_ERROR: u8 = 1;

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(code) => code,
        Err(err) => {
            output::error(&err);
            ExitCode::from(EXIT_ERROR)
        }
    }
}

fn run(cli: Cli) -> anyhow::Result<ExitCode> {
    let overrides = cli.overrides();
    let app = app::App::build(cli.root.as_deref(), &overrides)?;
    logging::init(app.log_filter(&overrides));
    tracing::debug!(root = %app.root.path.display(), source = %app.root.source, "project root");

    let format = app.output_format(&overrides);
    match cli.command {
        Command::Init => commands::init::run(&app, format),
        Command::Doctor => commands::doctor::run(&app, format),
        Command::Config => commands::config::run(&app, format),
    }
}
