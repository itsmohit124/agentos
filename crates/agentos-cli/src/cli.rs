//! Command-line argument definitions.

use std::path::PathBuf;

use agentos_config::{ConfigOverrides, OutputFormat};
use clap::{ArgAction, Args, Parser, Subcommand, ValueEnum};

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
    /// Discover, inspect, validate, install and graph skills.
    Skills(SkillsArgs),
    /// Print the derived project profile.
    Project(ProjectArgs),
    /// Index the repository into the knowledge graph.
    Scan,
    /// Query or rebuild the knowledge graph.
    Knowledge(KnowledgeArgs),
    /// Run a deterministic evaluation suite.
    Evaluate(EvaluateArgs),
}

/// `agentos skills` arguments.
#[derive(Debug, Args)]
pub struct SkillsArgs {
    /// User-level skills directory [default: $HOME/.agentos/skills].
    #[arg(
        long,
        global = true,
        env = "AGENTOS_USER_SKILLS_DIR",
        value_name = "DIR"
    )]
    pub user_skills_dir: Option<PathBuf>,

    /// Use this directory instead of the built-in skills shipped with agentos.
    #[arg(
        long,
        global = true,
        env = "AGENTOS_BUILTIN_SKILLS_DIR",
        value_name = "DIR"
    )]
    pub builtin_skills_dir: Option<PathBuf>,

    #[command(subcommand)]
    pub command: SkillsCommand,
}

/// `agentos skills` subcommands.
#[derive(Debug, Subcommand)]
pub enum SkillsCommand {
    /// List the skills that won discovery, highest-priority level first per name.
    List,
    /// Show a skill's metadata, resolved dependencies, permissions and body.
    Inspect {
        /// Skill name.
        name: String,
        /// Metadata only; do not read the skill body.
        #[arg(long)]
        brief: bool,
    },
    /// Validate every skill, including bodies, references and invocation rules.
    Validate,
    /// Copy a local skill directory into .agents/skills/ after validating it.
    Install {
        /// Local skill directory (git URLs are not supported yet).
        source: String,
    },
    /// Print the skill dependency graph.
    Graph {
        /// Output format.
        #[arg(long, value_enum, default_value_t = GraphFormat::Text)]
        format: GraphFormat,
    },
}

/// `agentos skills graph` formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum GraphFormat {
    /// Human-readable adjacency list.
    Text,
    /// Graphviz DOT.
    Dot,
}

/// `agentos project` arguments.
#[derive(Debug, Args)]
pub struct ProjectArgs {
    #[command(subcommand)]
    pub command: ProjectCommand,
}

/// `agentos project` subcommands.
#[derive(Debug, Subcommand)]
pub enum ProjectCommand {
    /// Print the persisted project profile.
    Profile,
}

/// `agentos knowledge` arguments.
#[derive(Debug, Args)]
pub struct KnowledgeArgs {
    #[command(subcommand)]
    pub command: KnowledgeCommand,
}

/// `agentos knowledge` subcommands.
#[derive(Debug, Subcommand)]
pub enum KnowledgeCommand {
    /// Counts of files, nodes, edges, fragments and provenance.
    Stats,
    /// List node ids and types.
    Nodes,
    /// List edges.
    Edges,
    /// Show one node with its edges and fragments.
    Inspect {
        /// Logical node id.
        id: String,
    },
    /// Delete the derived graph and scan again.
    Rebuild,
}

/// `agentos evaluate` arguments.
#[derive(Debug, Args)]
pub struct EvaluateArgs {
    /// Area to evaluate.
    #[arg(value_enum)]
    pub area: EvalArea,

    /// Directory with the case files [default: <root>/evals/<area>].
    #[arg(long, value_name = "DIR")]
    pub evals_dir: Option<PathBuf>,

    /// Fixtures directory [default: <root>/fixtures].
    #[arg(long, value_name = "DIR")]
    pub fixtures_dir: Option<PathBuf>,
}

/// Evaluation areas.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum EvalArea {
    /// The skill system (`evals/skills`).
    Skills,
    /// Project understanding and the knowledge graph (`evals/knowledge`).
    Knowledge,
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
