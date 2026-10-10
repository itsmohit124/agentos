//! `agentos plan` and `agentos run`.

use std::fs;
use std::path::Path;
use std::process::ExitCode;
use std::sync::Arc;

use agentos_config::OutputFormat;
use agentos_context::{ContextConfig, SqliteView};
use agentos_core::fs::RealFs;
use agentos_knowledge::{KnowledgeStore, domain_scan, scan};
use agentos_skills::{SkillLoader, builtin};
use agentos_workflow::{
    EngineContext, ManualRunner, MockRunner, Phase5Validator, Plan, PlanInput, RunRecord,
    RunnerResult, ScriptedValidator, Session, WorkflowError, plan as build_plan, start,
};
use anyhow::Context;

use crate::app::App;
use crate::cli::{PlanArgs, RunArgs, RunCommand, RunnerChoice};
use crate::output;

const EXIT_USAGE: u8 = 2;
const EXIT_CHECK: u8 = 3;

/// Runs `agentos plan`.
pub fn plan(app: &App, args: &PlanArgs, format: OutputFormat) -> anyhow::Result<ExitCode> {
    let registry = discover(&app.root.path);
    let planned = match build_plan(&PlanInput {
        task: &args.task,
        workflow: args.workflow.as_deref(),
        definition: None,
        registry: &registry,
        concurrency: None,
        max_fix_iterations: None,
    }) {
        Ok(plan) => plan,
        Err(WorkflowError::UnknownWorkflow(name)) => {
            output::error(&anyhow::anyhow!(
                "unknown workflow {name:?} (known: bugfix, feature, refactor)"
            ));
            return Ok(ExitCode::from(EXIT_USAGE));
        }
        Err(err) => return Err(err.into()),
    };
    agentos_workflow::save_plan(&app.root.path, &planned)?;
    match format {
        OutputFormat::Json => output::json(&planned)?,
        OutputFormat::Text => output::text(&render_plan(&planned))?,
    }
    Ok(ExitCode::SUCCESS)
}

/// Runs `agentos run`.
pub fn run(app: &App, args: &RunArgs, format: OutputFormat) -> anyhow::Result<ExitCode> {
    match &args.command {
        Some(RunCommand::Status { run_id }) => {
            let run = agentos_workflow::status(&app.root.path, run_id)?;
            emit_run(&run, format)?;
            Ok(exit_for(&run))
        }
        Some(RunCommand::Abort { run_id }) => {
            let run = agentos_workflow::abort(&app.root.path, run_id)?;
            emit_run(&run, format)?;
            Ok(ExitCode::SUCCESS)
        }
        Some(RunCommand::Resume {
            run_id,
            runner,
            result,
        }) => {
            let run = execute(app, ExecuteMode::Resume(run_id), *runner, result.as_deref())?;
            emit_run(&run, format)?;
            Ok(exit_for(&run))
        }
        None => {
            let Some(plan_id) = args.plan_id.as_deref() else {
                output::error(&anyhow::anyhow!(
                    "usage: agentos run <plan-id> | agentos run status|resume|abort <run-id>"
                ));
                return Ok(ExitCode::from(EXIT_USAGE));
            };
            let run = execute(
                app,
                ExecuteMode::Start(plan_id),
                args.runner,
                args.result.as_deref(),
            )?;
            emit_run(&run, format)?;
            Ok(exit_for(&run))
        }
    }
}

enum ExecuteMode<'a> {
    Start(&'a str),
    Resume(&'a str),
}

fn execute(
    app: &App,
    mode: ExecuteMode<'_>,
    choice: RunnerChoice,
    result: Option<&Path>,
) -> anyhow::Result<RunRecord> {
    let root = &app.root.path;
    scan(root).with_context(|| format!("cannot scan {}", root.display()))?;
    domain_scan(root)
        .with_context(|| format!("cannot scan domain knowledge in {}", root.display()))?;
    let store = KnowledgeStore::open(root)
        .with_context(|| format!("cannot open knowledge index in {}", root.display()))?;
    let view = SqliteView::new(&store);
    let context = EngineContext {
        view,
        config: ContextConfig {
            budget: None,
            cache_dir: None,
        },
    };
    match choice {
        RunnerChoice::Mock => {
            let mut runner = MockRunner::succeed();
            let mut validator = ScriptedValidator::pass();
            let mut session = Session {
                root,
                runner: &mut runner,
                validator: &mut validator,
                context: &context,
            };
            dispatch(&mut session, mode)
        }
        RunnerChoice::Manual => {
            let mut runner = ManualRunner::new();
            queue_manual(&mut runner, root, &mode, result)?;
            let mut validator = Phase5Validator::new(root);
            let mut session = Session {
                root,
                runner: &mut runner,
                validator: &mut validator,
                context: &context,
            };
            dispatch(&mut session, mode)
        }
    }
}

fn dispatch(session: &mut Session<'_>, mode: ExecuteMode<'_>) -> anyhow::Result<RunRecord> {
    match mode {
        ExecuteMode::Start(plan_id) => {
            let planned = agentos_workflow::load_plan(session.root, plan_id)?;
            start(session, &planned).map_err(Into::into)
        }
        ExecuteMode::Resume(run_id) => {
            agentos_workflow::resume(session, run_id).map_err(Into::into)
        }
    }
}

fn queue_manual(
    runner: &mut ManualRunner,
    root: &Path,
    mode: &ExecuteMode<'_>,
    result: Option<&Path>,
) -> anyhow::Result<()> {
    let Some(path) = result else {
        return Ok(());
    };
    let step_id = match mode {
        ExecuteMode::Resume(run_id) => agentos_workflow::status(root, run_id)?.pending_step,
        ExecuteMode::Start(plan_id) => agentos_workflow::load_plan(root, plan_id)?
            .steps
            .first()
            .map(|step| step.id.clone()),
    };
    let Some(step_id) = step_id else {
        return Ok(());
    };
    let text =
        fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
    let parsed: RunnerResult = serde_json::from_str(&text)
        .with_context(|| format!("{} is not a runner result", path.display()))?;
    runner.push(&step_id, parsed);
    Ok(())
}

fn discover(root: &Path) -> agentos_skills::SkillRegistry {
    SkillLoader::for_project(root, Arc::new(RealFs), None, builtin::location())
        .discover()
        .registry
}

fn exit_for(run: &RunRecord) -> ExitCode {
    if matches!(
        run.state,
        agentos_workflow::RunState::Failed | agentos_workflow::RunState::Blocked
    ) {
        ExitCode::from(EXIT_CHECK)
    } else {
        ExitCode::SUCCESS
    }
}

fn emit_run(run: &RunRecord, format: OutputFormat) -> anyhow::Result<()> {
    match format {
        OutputFormat::Json => output::json(run),
        OutputFormat::Text => output::text(&render_run(run)),
    }
}

fn render_plan(plan: &Plan) -> String {
    let mut out = String::new();
    out.push_str(&format!("plan: {}\n", plan.id));
    out.push_str(&format!("task: {}\n", plan.task));
    out.push_str(&format!(
        "workflow: {} ({}, {})\n",
        plan.workflow, plan.selection.source, plan.selection.rule
    ));
    out.push_str(&format!(
        "concurrency: {}\nmax_fix_iterations: {}\n",
        plan.execution.concurrency, plan.execution.max_fix_iterations
    ));
    out.push_str("steps:\n");
    for step in &plan.steps {
        out.push_str(&format!(
            "  {} depends={} ready_with={}\n",
            step.id,
            step.depends_on.join(","),
            step.ready_with.join(",")
        ));
    }
    out.push_str("skipped:\n");
    for step in &plan.skipped {
        out.push_str(&format!(
            "  skipped: {}\n  reason: {}\n",
            step.id, step.reason
        ));
    }
    out.push_str("skills:\n");
    for skill in &plan.skills {
        out.push_str(&format!(
            "  {} step={} {}\n",
            skill.name, skill.step_id, skill.reason
        ));
    }
    out.push_str("permissions:\n");
    for cap in &plan.permissions {
        out.push_str(&format!("  {cap}\n"));
    }
    out.push_str("validation:\n");
    for item in &plan.validation {
        out.push_str(&format!(
            "  {} kind={} fix_loop={} max_iterations={}\n",
            item.step_id, item.kind, item.fix_loop, item.max_iterations
        ));
    }
    out
}

fn render_run(run: &RunRecord) -> String {
    let mut out = String::new();
    out.push_str(&format!("run: {}\n", run.id));
    out.push_str(&format!("plan: {}\n", run.plan_id));
    out.push_str(&format!("state: {}\n", run.state));
    if run.interrupted {
        out.push_str("interrupted: true\n");
    }
    if run.waiting {
        out.push_str("waiting: true\n");
    }
    if let Some(step) = &run.pending_step {
        out.push_str(&format!("pending: {step}\n"));
    }
    if let Some(reason) = &run.failure_reason {
        out.push_str(&format!("reason: {reason}\n"));
    }
    out.push_str("steps:\n");
    for step in &run.steps {
        out.push_str(&format!("  {} {}\n", step.id, step.state));
    }
    out.push_str("skipped:\n");
    for step in &run.skipped {
        out.push_str(&format!(
            "  skipped: {}\n  reason: {}\n",
            step.id, step.reason
        ));
    }
    if let Some(text) = &run.pending_instructions {
        out.push_str("instructions:\n");
        out.push_str(text);
        if !text.ends_with('\n') {
            out.push('\n');
        }
    }
    out
}
