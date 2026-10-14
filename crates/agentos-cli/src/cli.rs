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
    /// Domain lifecycle: contexts, terminology, proposals, confirmation.
    Domain(DomainArgs),
    /// Measure naive context costs. Does not build a Context Pack.
    Benchmark(BenchmarkArgs),
    /// Build a Context Pack from the project knowledge graph.
    Context(ContextArgs),
    /// Run validation providers.
    Validate(ValidateArgs),
    /// Analyze the knowledge graph for architecture smells.
    Architecture(ArchitectureArgs),
    /// Plan a task into a deterministic workflow.
    Plan(PlanArgs),
    /// Execute, resume, abort, or inspect a run.
    Run(RunArgs),
    /// Inspect and record project memory.
    Memory(MemoryArgs),
    /// Install and inspect coding-agent integrations.
    Adapter(AdapterArgs),
    /// Run a deterministic evaluation suite.
    Evaluate(EvaluateArgs),
    /// Remove `.agentos/` and managed adapter blocks after confirmation.
    Cleanup(CleanupArgs),
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

/// `agentos domain` arguments.
#[derive(Debug, Args)]
pub struct DomainArgs {
    #[command(subcommand)]
    pub command: DomainCommand,
}

/// `agentos domain` subcommands.
#[derive(Debug, Subcommand)]
pub enum DomainCommand {
    /// Infer domain nodes from the scanned graph. Does not confirm anything.
    Scan,
    /// List bounded contexts.
    Contexts,
    /// List ubiquitous-language terms and ambiguity findings.
    Terminology,
    /// List context-map edges.
    Map,
    /// Move inferred nodes one step toward Proposed.
    Propose,
    /// Confirm a Proposed node, or promote a Confirmed node to Canonical.
    Confirm {
        /// Logical domain node id.
        id: String,
        /// Who is confirming. Default: operator.
        #[arg(long)]
        by: Option<String>,
        /// Why this confirmation is being recorded.
        #[arg(long)]
        reason: Option<String>,
    },
    /// Reject a domain node. Canonical nodes cannot be rejected.
    Reject {
        /// Logical domain node id.
        id: String,
        /// Who is rejecting. Default: operator.
        #[arg(long)]
        by: Option<String>,
        /// Why this rejection is being recorded.
        #[arg(long)]
        reason: Option<String>,
    },
    /// Fail when canonical evidence is missing or a lifecycle rule is broken.
    Validate,
}

/// `agentos benchmark` arguments.
#[derive(Debug, Args)]
pub struct BenchmarkArgs {
    #[command(subcommand)]
    pub command: BenchmarkCommand,
}

/// `agentos benchmark` subcommands.
#[derive(Debug, Subcommand)]
pub enum BenchmarkCommand {
    /// Count full-repository and manually chosen file tokens for each golden task.
    Baseline {
        /// Fixture directory, relative to the project root or absolute.
        fixture: PathBuf,
    },
}

/// `agentos context` arguments.
#[derive(Debug, Args)]
pub struct ContextArgs {
    /// Token budget. Omit it to keep every relevant item without dumping the repository.
    #[arg(long)]
    pub budget: Option<u32>,

    #[command(subcommand)]
    pub command: ContextCommand,
}

/// `agentos context` subcommands.
#[derive(Debug, Subcommand)]
pub enum ContextCommand {
    /// Explain which contexts and terms a task routes to.
    Explain {
        /// Task text.
        task: String,
    },
    /// Build a Context Pack.
    Build {
        /// Task text.
        task: String,
        /// Pack format.
        #[arg(long, value_enum, default_value_t = PackFormat::Md)]
        format: PackFormat,
    },
    /// Summarise a Context Pack.
    Stats {
        /// Task text.
        task: String,
    },
}

/// `agentos context build` formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum PackFormat {
    /// Pretty JSON.
    Json,
    /// Markdown.
    Md,
}

/// `agentos validate` arguments.
#[derive(Debug, Args)]
pub struct ValidateArgs {
    /// Kind to run. Ignored by `detect`, `last`, and `record-regression`.
    #[arg(long, value_enum)]
    pub kind: Option<ValidateKind>,

    /// Task id stored on the result and its feedback items.
    #[arg(long)]
    pub task: Option<String>,

    /// Context Pack id stored on the result and its feedback items.
    #[arg(long)]
    pub context_pack: Option<String>,

    #[command(subcommand)]
    pub command: Option<ValidateCommand>,
}

/// `agentos validate` subcommands. Absent means run validation.
#[derive(Debug, Subcommand)]
pub enum ValidateCommand {
    /// List providers selected for this project.
    Detect,
    /// Print the most recently stored validation result.
    Last,
    /// Promote a failed result to `evals/feedback/<id>.json`.
    RecordRegression {
        /// Stored validation id.
        id: String,
    },
}

/// Validation kinds accepted by `--kind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "snake_case")]
pub enum ValidateKind {
    /// Static type check.
    Typecheck,
    /// Lint.
    Lint,
    /// Unit tests.
    UnitTest,
    /// Integration tests.
    IntegrationTest,
    /// Build or package.
    Build,
    /// Architecture detectors. Use `architecture analyze` for the report.
    Architecture,
    /// Browser validation. Requires the `browser` capability.
    Browser,
}

/// `agentos architecture` arguments.
#[derive(Debug, Args)]
pub struct ArchitectureArgs {
    #[command(subcommand)]
    pub command: ArchitectureCommand,
}

/// `agentos architecture` subcommands.
#[derive(Debug, Subcommand)]
pub enum ArchitectureCommand {
    /// Run deterministic detectors over the project knowledge graph.
    Analyze {
        /// Directory to analyze. Defaults to `--root` (the project root).
        path: Option<PathBuf>,
    },
}

/// `agentos plan` arguments.
#[derive(Debug, Args)]
pub struct PlanArgs {
    /// Task text. It is stored unchanged.
    pub task: String,
    /// Workflow to run. Omit it to select feature, bugfix, or refactor from task-type words.
    #[arg(long, value_name = "NAME")]
    pub workflow: Option<String>,
}

/// `agentos run` arguments.
#[derive(Debug, Args)]
#[command(args_conflicts_with_subcommands = true)]
pub struct RunArgs {
    /// Status, resume, or abort. Omit this to execute a plan.
    #[command(subcommand)]
    pub command: Option<RunCommand>,

    /// Plan id to execute.
    pub plan_id: Option<String>,

    /// `mock` succeeds deterministically. `manual` prints the step and records `--result`.
    #[arg(long, value_enum, default_value_t = RunnerChoice::Manual)]
    pub runner: RunnerChoice,

    /// JSON runner result for the manual runner.
    #[arg(long, value_name = "PATH")]
    pub result: Option<PathBuf>,
}

/// `agentos run` subcommands.
#[derive(Debug, Subcommand)]
pub enum RunCommand {
    /// Show a run.
    Status {
        /// Run id.
        run_id: String,
    },
    /// Continue a run from its saved state.
    Resume {
        /// Run id.
        run_id: String,
        /// Runner to use for the remaining steps.
        #[arg(long, value_enum, default_value_t = RunnerChoice::Manual)]
        runner: RunnerChoice,
        /// JSON runner result for the manual runner.
        #[arg(long, value_name = "PATH")]
        result: Option<PathBuf>,
    },
    /// Abort a run that is not already finished.
    Abort {
        /// Run id.
        run_id: String,
    },
}

/// How `agentos run` executes a step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum RunnerChoice {
    /// Scripted success. Validation passes without spawning a tool.
    Mock,
    /// Print the step instructions and Context Pack, then record `--result`.
    Manual,
}

/// `agentos memory` arguments.
#[derive(Debug, Args)]
pub struct MemoryArgs {
    #[command(subcommand)]
    pub command: MemoryCommand,
}

/// `agentos memory` subcommands.
#[derive(Debug, Subcommand)]
pub enum MemoryCommand {
    /// List memory records, including forgotten ones.
    List,
    /// Show one memory record.
    Show {
        /// Memory id.
        id: String,
    },
    /// Add a memory record. It is proposed unless `--confirm` is set.
    Add {
        /// `episodic`, `semantic`, `procedural`, `decision`, or `lesson`.
        #[arg(long, value_enum)]
        r#type: MemoryTypeArg,
        /// Short name.
        #[arg(long)]
        name: String,
        /// Body. Secret-like patterns are redacted before they are stored.
        #[arg(long)]
        content: String,
        /// Task this memory is about.
        #[arg(long, default_value = "")]
        task: String,
        /// Run id. Empty when the record did not come from a run.
        #[arg(long, default_value = "")]
        run: String,
        /// Outcome such as `failed` or `completed`.
        #[arg(long, default_value = "")]
        outcome: String,
        /// Link `kind|id|fingerprint`. `kind` is `source`, `domain`, or `symbol`. Repeatable.
        #[arg(long = "link", value_name = "KIND|ID|FINGERPRINT")]
        links: Vec<String>,
        /// Timestamp `YYYY-MM-DDTHH:MM:SSZ`. Default: current UTC time.
        #[arg(long)]
        at: Option<String>,
        /// Store the record as confirmed. Omit it to store `proposed`.
        #[arg(long)]
        confirm: bool,
    },
    /// Confirm a proposed memory. This does not make it canonical.
    Confirm {
        /// Memory id.
        id: String,
        /// Who is confirming. Default: operator.
        #[arg(long)]
        by: Option<String>,
        /// Timestamp. Default: current UTC time.
        #[arg(long)]
        at: Option<String>,
    },
    /// Forget a memory. Retrieval will not return it.
    Forget {
        /// Memory id.
        id: String,
        /// Timestamp. Default: current UTC time.
        #[arg(long)]
        at: Option<String>,
    },
    /// Delete stale memories. Fresh records are kept.
    Prune {
        /// Required. Deletes records whose linked fingerprints no longer match.
        #[arg(long, required = true)]
        stale: bool,
        /// Timestamp. Default: current UTC time.
        #[arg(long)]
        at: Option<String>,
    },
    /// Ask the Context Engine which memories it would select for a task.
    Query {
        /// Task text.
        task: String,
    },
}

/// Memory types accepted by `memory add --type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum MemoryTypeArg {
    /// What happened during a task or run.
    Episodic,
    /// A stable fact about the project.
    Semantic,
    /// How work is performed, including a context pattern.
    Procedural,
    /// An explicit engineering or domain decision.
    Decision,
    /// Knowledge derived from a failure or a review.
    Lesson,
}

impl MemoryTypeArg {
    /// Wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Episodic => "episodic",
            Self::Semantic => "semantic",
            Self::Procedural => "procedural",
            Self::Decision => "decision",
            Self::Lesson => "lesson",
        }
    }
}

/// `agentos adapter` arguments.
#[derive(Debug, Args)]
pub struct AdapterArgs {
    #[command(subcommand)]
    pub command: AdapterCommand,
}

/// `agentos adapter` subcommands.
#[derive(Debug, Subcommand)]
pub enum AdapterCommand {
    /// List adapters and their capabilities. Does not write files.
    List,
    /// Install one adapter into the project.
    Install {
        /// `generic`, `cursor`, `claude`, or `codex`.
        adapter: String,
    },
    /// Remove AgentOS-managed files for one adapter.
    Uninstall {
        /// `generic`, `cursor`, `claude`, or `codex`.
        adapter: String,
    },
    /// Check one adapter without modifying the project.
    Doctor {
        /// `generic`, `cursor`, `claude`, or `codex`.
        adapter: String,
    },
}

/// `agentos evaluate` arguments.
#[derive(Debug, Args)]
pub struct EvaluateArgs {
    /// Area to evaluate. Omit it to print the dashboard.
    #[arg(value_enum)]
    pub area: Option<EvalArea>,

    /// Case directory for one area, or the `evals/` directory for the dashboard and `e2e`.
    #[arg(long, value_name = "DIR")]
    pub evals_dir: Option<PathBuf>,

    /// Fixtures directory [default: <root>/fixtures].
    #[arg(long, value_name = "DIR")]
    pub fixtures_dir: Option<PathBuf>,
}

/// `agentos cleanup` arguments.
#[derive(Debug, Args)]
pub struct CleanupArgs {
    /// Confirm removal of `.agentos/` and managed adapter blocks.
    #[arg(long)]
    pub yes: bool,
}

/// Evaluation areas.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum EvalArea {
    /// The skill system (`evals/skills`).
    Skills,
    /// Project understanding and the knowledge graph (`evals/knowledge`).
    Knowledge,
    /// Domain lifecycle and DDD fixtures (`evals/ddd`).
    Ddd,
    /// Context packs (`evals/context`).
    Context,
    /// Validation providers (`evals/feedback`).
    Feedback,
    /// Architecture detectors (`evals/architecture`).
    Architecture,
    /// Workflow plans and runs (`evals/workflow`).
    Workflow,
    /// Project memory (`evals/memory`).
    Memory,
    /// Agent adapters (`evals/adapters`).
    Adapters,
    /// Golden workflows (`evals/end-to-end`).
    E2e,
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

    #[test]
    fn run_accepts_a_plan_id_or_a_subcommand() {
        let start =
            Cli::try_parse_from(["agentos", "run", "feature-abc-1", "--runner", "mock"]).unwrap();
        match start.command {
            Command::Run(args) => {
                assert_eq!(args.plan_id.as_deref(), Some("feature-abc-1"));
                assert!(args.command.is_none());
                assert_eq!(args.runner, RunnerChoice::Mock);
            }
            other => panic!("unexpected {other:?}"),
        }
        let status = Cli::try_parse_from(["agentos", "run", "status", "run-1"]).unwrap();
        match status.command {
            Command::Run(args) => match args.command {
                Some(RunCommand::Status { run_id }) => assert_eq!(run_id, "run-1"),
                other => panic!("unexpected {other:?}"),
            },
            other => panic!("unexpected {other:?}"),
        }
    }
}
