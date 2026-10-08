//! Command-line argument definitions.

use std::path::PathBuf;

use agentos_config::{ConfigOverrides, OutputFormat};
use clap::{ArgAction, Parser, Subcommand};

/// AgentOS: local-first engineering intelligence layer for coding agents.
#[derive(Debug, Parser)]
#[command(name = "agentos", version, propagate_version = true)]
pub struct Cli {
    /// Project root (default: Git root of the current directory, else the current directory).
    #[arg(long, global = true, value_name = "PATH")]
    pub root: Option<PathBuf>,

    /// Increase log verbosity (-v debug, -vv trace). Overrides AGENTOS_LOG and the config file.
    #[arg(short, long, global = true, action = ArgAction::Count, conflicts_with = "log_level")]
    pub verbose: u8,

    /// Log filter, e.g. "info" or "agentos_core=debug,warn". Overrides AGENTOS_LOG.
    #[arg(long, global = true, value_name = "FILTER")]
    pub log_level: Option<String>,

    /// Emit machine-readable JSON on stdout.
    #[arg(long, global = true)]
    pub json: bool,

    #[command(subcommand)]
    pub command: Command,
}

/// Subcommands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Create the .agentos/ project state (idempotent).
    Init,
    /// Check the AgentOS installation and project state.
    Doctor,
    /// Show the effective configuration and where each value came from.
    Config,
}

impl Cli {
    /// Config overrides expressed by CLI flags.
    pub fn overrides(&self) -> ConfigOverrides {
        let log_level = match self.verbose {
            0 => self.log_level.clone(),
            1 => Some("debug".to_owned()),
            _ => Some("trace".to_owned()),
        };
        ConfigOverrides {
            log_level,
            output_format: self.json.then_some(OutputFormat::Json),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn verbose_maps_to_log_levels() {
        let parse = |args: &[&str]| Cli::parse_from(args).overrides().log_level;
        assert_eq!(parse(&["agentos", "doctor"]), None);
        assert_eq!(parse(&["agentos", "-v", "doctor"]), Some("debug".into()));
        assert_eq!(parse(&["agentos", "doctor", "-vv"]), Some("trace".into()));
        assert_eq!(
            parse(&["agentos", "--log-level", "info", "doctor"]),
            Some("info".into())
        );
    }

    #[test]
    fn verbose_conflicts_with_log_level() {
        assert!(Cli::try_parse_from(["agentos", "-v", "--log-level", "info", "doctor"]).is_err());
    }

    #[test]
    fn json_flag_is_global() {
        let cli = Cli::parse_from(["agentos", "doctor", "--json"]);
        assert_eq!(cli.overrides().output_format, Some(OutputFormat::Json));
        let cli = Cli::parse_from(["agentos", "config"]);
        assert_eq!(cli.overrides().output_format, None);
    }
}
