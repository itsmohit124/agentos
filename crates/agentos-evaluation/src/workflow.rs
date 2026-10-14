//! Workflow evaluation. Cases plan and run with [`MockRunner`]. No model or network is used.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use agentos_context::{ContextConfig, MemoryView};
use agentos_core::fs::{FileSystem, MemFs, RealFs};
use agentos_knowledge::{KnowledgeFragment, KnowledgeNode, NodeKind, Provenance, ProvenanceKind};
use agentos_skills::{SkillLoader, SkillRegistry, builtin};
use agentos_workflow::{
    EngineContext, MockReaction, MockRunner, PlanInput, RunState, Scope, ScriptedValidator,
    Session, StepSpec, TestEdit, WorkflowDefinition, allows, plan, start,
};
use serde::Deserialize;

use crate::{CaseResult, EvalError, EvalReport, EvalTrace, TraceExclusion};

/// Runs every case in `cases_dir`. `fixtures_dir` is unused; the cases are synthetic.
pub fn run(cases_dir: &Path, _fixtures_dir: &Path) -> Result<EvalReport, EvalError> {
    let fs = RealFs;
    if !fs.is_dir(cases_dir) {
        return Err(EvalError::NoCases(cases_dir.to_path_buf()));
    }
    let mut files: Vec<PathBuf> = fs
        .read_dir(cases_dir)?
        .into_iter()
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    files.sort();
    if files.is_empty() {
        return Err(EvalError::NoCases(cases_dir.to_path_buf()));
    }
    let mut cases = Vec::new();
    for path in files {
        let text = fs.read_to_string(&path)?;
        let case: Case = serde_json::from_str(&text).map_err(|err| EvalError::InvalidCase {
            path: path.clone(),
            message: err.to_string(),
        })?;
        let stem = path
            .file_stem()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        if case.id != stem {
            return Err(EvalError::InvalidCase {
                path,
                message: format!("id {:?} must equal the file name {stem:?}", case.id),
            });
        }
        if case.description.trim().is_empty() {
            return Err(EvalError::InvalidCase {
                path,
                message: "description must not be empty".to_owned(),
            });
        }
        cases.push(execute(&case));
    }
    Ok(EvalReport {
        area: "workflow".to_owned(),
        cases_dir: cases_dir.to_path_buf(),
        cases,
    })
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    id: String,
    kind: String,
    description: String,
    #[serde(default)]
    workflow: Option<String>,
    #[serde(default)]
    task: String,
    #[serde(default)]
    concurrency: Option<u32>,
    #[serde(default)]
    max_fix_iterations: Option<u32>,
    #[serde(default)]
    steps: Vec<StepJson>,
    #[serde(default)]
    expect_waves: Vec<Vec<String>>,
    #[serde(default)]
    expect_frontier: Vec<String>,
    #[serde(default)]
    expect_error: Option<String>,
    #[serde(default)]
    expect_skipped: Vec<SkipJson>,
    #[serde(default)]
    expect_state: Option<String>,
    #[serde(default)]
    expect_calls: Vec<String>,
    #[serde(default)]
    validations: Vec<bool>,
    #[serde(default)]
    weaken: Option<String>,
    #[serde(default)]
    include_fragment: Option<String>,
    #[serde(default)]
    exclude_fragment: Option<String>,
    #[serde(default)]
    legal_run: Vec<Vec<String>>,
    #[serde(default)]
    legal_step: Vec<Vec<String>>,
    #[serde(default)]
    suspend_after: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StepJson {
    id: String,
    #[serde(default)]
    depends_on: Vec<String>,
    #[serde(default)]
    consumes: Vec<String>,
    #[serde(default)]
    outputs: Vec<String>,
    #[serde(default)]
    skill: Option<String>,
    #[serde(default)]
    validation: Option<String>,
    #[serde(default)]
    permissions: Vec<String>,
    #[serde(default)]
    fix_loop: bool,
    #[serde(default)]
    skip_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SkipJson {
    id: String,
    reason: String,
}

fn execute(case: &Case) -> CaseResult {
    let detail = match case.kind.as_str() {
        "graph" => graph_case(case),
        "error" => error_case(case),
        "permission" => permission_case(case),
        "resume" => resume_case(case),
        "abort" => abort_case(case),
        "context" => context_case(case),
        "fix" => fix_case(case),
        "weaken" => weaken_case(case),
        "transition" => transition_case(case),
        "workflow" => workflow_case(case),
        "deterministic" => deterministic_case(case),
        "skill_cycle" => skill_cycle_case(case),
        other => Err(format!("unknown kind {other}")),
    };
    let trace = trace_from_case(case);
    match detail {
        Ok(detail) => CaseResult {
            id: case.id.clone(),
            kind: case.kind.clone(),
            description: case.description.clone(),
            passed: true,
            detail,
            trace: Some(trace),
        },
        Err(detail) => CaseResult {
            id: case.id.clone(),
            kind: case.kind.clone(),
            description: case.description.clone(),
            passed: false,
            detail,
            trace: Some(trace),
        },
    }
}

fn trace_from_case(case: &Case) -> EvalTrace {
    let mut inputs = vec![
        format!("kind={}", case.kind),
        format!("task={}", case.task),
        "runner=MockRunner".to_owned(),
    ];
    if let Some(workflow) = &case.workflow {
        inputs.push(format!("workflow={workflow}"));
    }
    if let Some(limit) = case.concurrency {
        inputs.push(format!("concurrency={limit}"));
    }
    let mut selected: Vec<String> = case
        .steps
        .iter()
        .filter(|step| step.skip_reason.is_none())
        .map(|step| step.id.clone())
        .collect();
    selected.sort();
    let mut excluded: Vec<TraceExclusion> = case
        .steps
        .iter()
        .filter_map(|step| {
            step.skip_reason.as_ref().map(|reason| TraceExclusion {
                id: step.id.clone(),
                reason: reason.clone(),
            })
        })
        .chain(case.expect_skipped.iter().map(|skip| TraceExclusion {
            id: skip.id.clone(),
            reason: skip.reason.clone(),
        }))
        .collect();
    excluded.sort_by(|left, right| left.id.cmp(&right.id).then(left.reason.cmp(&right.reason)));
    excluded.dedup();
    let mut tools: Vec<String> = case
        .steps
        .iter()
        .filter_map(|step| step.validation.clone())
        .collect();
    tools.sort();
    tools.dedup();
    let mut permissions: Vec<String> = case
        .steps
        .iter()
        .flat_map(|step| step.permissions.clone())
        .collect();
    permissions.sort();
    permissions.dedup();
    let validation: Vec<String> = case
        .validations
        .iter()
        .enumerate()
        .map(|(index, pass)| {
            format!(
                "scripted attempt {}: {}",
                index + 1,
                if *pass { "pass" } else { "fail" }
            )
        })
        .collect();
    EvalTrace {
        inputs,
        selected,
        excluded,
        tools,
        permissions,
        validation,
    }
}

fn graph_case(case: &Case) -> Result<String, String> {
    let run = run_definition(case, &definition_from(case)?, MockRunner::succeed(), &[])?;
    if run.waves != case.expect_waves {
        return Err(format!("waves {:?} != {:?}", run.waves, case.expect_waves));
    }
    if !case.expect_frontier.is_empty()
        && !run
            .frontiers
            .iter()
            .any(|frontier| frontier == &case.expect_frontier)
    {
        return Err(format!(
            "frontier {:?} missing from {:?}",
            case.expect_frontier, run.frontiers
        ));
    }
    if let Some(limit) = case.concurrency
        && run
            .waves
            .iter()
            .any(|wave| wave.len() > usize::try_from(limit).unwrap_or(1))
    {
        return Err(format!(
            "a wave exceeded concurrency {limit}: {:?}",
            run.waves
        ));
    }
    expect_state(&run.state, case)?;
    Ok(format!("waves {}", run.waves.len()))
}

fn error_case(case: &Case) -> Result<String, String> {
    let registry = empty_registry();
    let definition = definition_from(case)?;
    let err = plan(&PlanInput {
        task: task_of(case),
        workflow: None,
        definition: Some(&definition),
        registry: &registry,
        concurrency: case.concurrency,
        max_fix_iterations: case.max_fix_iterations,
    })
    .map(|_| "planned".to_owned())
    .unwrap_or_else(|err| err.to_string());
    expect_contains(&err, case.expect_error.as_deref().unwrap_or(""))?;
    Ok(err)
}

fn permission_case(case: &Case) -> Result<String, String> {
    let run = run_definition(case, &definition_from(case)?, MockRunner::succeed(), &[])?;
    if !run.executed.is_empty() {
        return Err(format!("runner was called: {:?}", run.executed));
    }
    expect_state(&run.state, case)?;
    let reason = run.failure_reason.clone().unwrap_or_default();
    if let Some(expected) = &case.expect_error {
        expect_contains(&reason, expected)?;
    }
    Ok(reason)
}

fn resume_case(case: &Case) -> Result<String, String> {
    let definition = definition_from(case)?;
    let planned = plan_of(case, &definition, &empty_registry())?;
    let root = tempfile::tempdir().map_err(|err| err.to_string())?;
    let mut runner = MockRunner::succeed();
    if let Some(step) = &case.suspend_after {
        runner = runner.suspend_after(step);
    }
    let mut validator = ScriptedValidator::pass();
    let context = empty_context();
    let run = {
        let mut session = Session {
            root: root.path(),
            runner: &mut runner,
            validator: &mut validator,
            context: &context,
        };
        start(&mut session, &planned).map_err(|err| err.to_string())?
    };
    let before = runner.calls.clone();
    let run = {
        let mut session = Session {
            root: root.path(),
            runner: &mut runner,
            validator: &mut validator,
            context: &context,
        };
        agentos_workflow::resume(&mut session, &run.id).map_err(|err| err.to_string())?
    };
    if runner.calls != case.expect_calls {
        return Err(format!(
            "calls {:?} != {:?}",
            runner.calls, case.expect_calls
        ));
    }
    let resumed: Vec<&str> = runner
        .calls
        .iter()
        .skip(before.len())
        .map(String::as_str)
        .collect();
    if resumed
        .iter()
        .any(|call| before.iter().any(|earlier| earlier == call))
    {
        return Err(format!("resume repeated {resumed:?}"));
    }
    expect_state(&run.state, case)?;
    Ok(format!("resumed {resumed:?}"))
}

fn abort_case(case: &Case) -> Result<String, String> {
    let definition = definition_from(case)?;
    let planned = plan_of(case, &definition, &empty_registry())?;
    let root = tempfile::tempdir().map_err(|err| err.to_string())?;
    let mut runner =
        MockRunner::succeed().suspend_after(case.suspend_after.as_deref().unwrap_or("a"));
    let mut validator = ScriptedValidator::pass();
    let context = empty_context();
    let run = {
        let mut session = Session {
            root: root.path(),
            runner: &mut runner,
            validator: &mut validator,
            context: &context,
        };
        start(&mut session, &planned).map_err(|err| err.to_string())?
    };
    let aborted = agentos_workflow::abort(root.path(), &run.id).map_err(|err| err.to_string())?;
    expect_state(&aborted.state, case)?;
    let err = agentos_workflow::abort(root.path(), &run.id)
        .map(|_| "aborted twice".to_owned())
        .unwrap_or_else(|err| err.to_string());
    expect_contains(
        &err,
        case.expect_error.as_deref().unwrap_or("illegal transition"),
    )?;
    Ok(err)
}

fn context_case(case: &Case) -> Result<String, String> {
    let definition = definition_from(case)?;
    let registry = empty_registry();
    let planned = plan_of(case, &definition, &registry)?;
    let view = MemoryView {
        nodes: vec![
            node(
                "domain.payments.bounded_context.Payments",
                NodeKind::BoundedContext,
                "Payments",
            ),
            node(
                "software.app.payments.function.Webhook",
                NodeKind::Function,
                "Webhook",
            ),
            node(
                "domain.cooking.bounded_context.Cooking",
                NodeKind::BoundedContext,
                "Cooking",
            ),
            node(
                "software.app.cooking.function.Recipe",
                NodeKind::Function,
                "Recipe",
            ),
        ],
        fragments: vec![
            fragment(
                "software.app.payments.function.Webhook",
                "retry the payment webhook",
            ),
            fragment(
                "software.app.cooking.function.Recipe",
                "boil pasta with butter",
            ),
        ],
        ..Default::default()
    };
    let root = tempfile::tempdir().map_err(|err| err.to_string())?;
    let mut runner = MockRunner::succeed();
    let mut validator = ScriptedValidator::pass();
    let context = EngineContext {
        view,
        config: ContextConfig {
            budget: None,
            cache_dir: None,
        },
    };
    let run = {
        let mut session = Session {
            root: root.path(),
            runner: &mut runner,
            validator: &mut validator,
            context: &context,
        };
        start(&mut session, &planned).map_err(|err| err.to_string())?
    };
    let ids: Vec<&str> = run
        .steps
        .iter()
        .map(|step| step.context_pack_id.as_deref().unwrap_or(""))
        .collect();
    if ids.iter().any(|id| id.is_empty()) {
        return Err("a step has no context pack".to_owned());
    }
    let mut unique = ids.to_vec();
    unique.sort_unstable();
    unique.dedup();
    if unique.len() != run.steps.len() {
        return Err(format!("context pack ids are not unique: {ids:?}"));
    }
    for step in &run.steps {
        let task = step.context_task.as_deref().unwrap_or("");
        if !task.contains(&format!("Step: {}", step.id)) {
            return Err(format!("step {} pack task is {task}", step.id));
        }
    }
    let fragments = &run.steps[0].selected_fragment_ids;
    if let Some(needle) = &case.include_fragment
        && !fragments.iter().any(|id| id.contains(needle))
    {
        return Err(format!("missing fragment {needle} in {fragments:?}"));
    }
    if let Some(needle) = &case.exclude_fragment
        && fragments.iter().any(|id| id.contains(needle))
    {
        return Err(format!("unexpected fragment {needle} in {fragments:?}"));
    }
    if run.steps.len() > 1 {
        let second = &run.steps[1];
        if !second
            .dependency_outputs
            .keys()
            .any(|key| key.starts_with(&format!("{}.", run.steps[0].id)))
            && !run.steps[0].outputs.is_empty()
        {
            return Err(format!(
                "dependency outputs missing: {:?}",
                second.dependency_outputs
            ));
        }
    }
    Ok(format!("packs {}", unique.len()))
}

fn fix_case(case: &Case) -> Result<String, String> {
    let run = run_definition(
        case,
        &definition_from(case)?,
        MockRunner::succeed(),
        &case.validations,
    )?;
    expect_state(&run.state, case)?;
    let reason = run
        .failure_reason
        .clone()
        .unwrap_or_else(|| run.state.to_string());
    if let Some(expected) = &case.expect_error {
        expect_contains(&reason, expected)?;
    }
    Ok(reason)
}

fn weaken_case(case: &Case) -> Result<String, String> {
    let edit = weaken_edit(case.weaken.as_deref().unwrap_or("delete"))?;
    let runner = MockRunner::succeed().script("check", vec![MockReaction::edits(vec![edit])]);
    let run = run_definition(case, &definition_from(case)?, runner, &[false, true])?;
    expect_state(&run.state, case)?;
    let reason = run.failure_reason.unwrap_or_default();
    expect_contains(
        &reason,
        case.expect_error.as_deref().unwrap_or("test weakening"),
    )?;
    Ok(reason)
}

fn transition_case(case: &Case) -> Result<String, String> {
    check_table(Scope::Run, &case.legal_run)?;
    check_table(Scope::Step, &case.legal_step)?;
    Ok(format!(
        "run {} step {}",
        case.legal_run.len(),
        case.legal_step.len()
    ))
}

fn workflow_case(case: &Case) -> Result<String, String> {
    let name = case
        .workflow
        .as_deref()
        .ok_or("workflow case needs a workflow")?;
    let registry = builtin_registry();
    let planned = plan(&PlanInput {
        task: task_of(case),
        workflow: Some(name),
        definition: None,
        registry: &registry,
        concurrency: None,
        max_fix_iterations: None,
    })
    .map_err(|err| err.to_string())?;
    if planned.task != task_of(case) {
        return Err("task text changed".to_owned());
    }
    for skipped in &case.expect_skipped {
        let found = planned
            .skipped
            .iter()
            .find(|step| step.id == skipped.id)
            .ok_or_else(|| format!("did not skip {}", skipped.id))?;
        if found.reason != skipped.reason {
            return Err(format!(
                "{} reason {:?} != {:?}",
                skipped.id, found.reason, skipped.reason
            ));
        }
    }
    let root = tempfile::tempdir().map_err(|err| err.to_string())?;
    let mut runner = MockRunner::succeed();
    let mut validator = ScriptedValidator::pass();
    let context = empty_context();
    let run = {
        let mut session = Session {
            root: root.path(),
            runner: &mut runner,
            validator: &mut validator,
            context: &context,
        };
        start(&mut session, &planned).map_err(|err| err.to_string())?
    };
    expect_state(&run.state, case)?;
    if run.steps.iter().any(|step| step.context_pack_id.is_none()) {
        return Err("an executed step has no context pack".to_owned());
    }
    if !root
        .path()
        .join(".agentos/runs")
        .join(&run.id)
        .join("run.json")
        .is_file()
    {
        return Err("run.json was not written".to_owned());
    }
    Ok(format!("{} {}", planned.id, run.state))
}

fn deterministic_case(case: &Case) -> Result<String, String> {
    let registry = builtin_registry();
    let task = task_of(case);
    let first = plan(&PlanInput {
        task,
        workflow: None,
        definition: None,
        registry: &registry,
        concurrency: None,
        max_fix_iterations: None,
    })
    .map_err(|err| err.to_string())?;
    let second = plan(&PlanInput {
        task,
        workflow: None,
        definition: None,
        registry: &registry,
        concurrency: None,
        max_fix_iterations: None,
    })
    .map_err(|err| err.to_string())?;
    if first != second {
        return Err("plans differ".to_owned());
    }
    if first.task != task {
        return Err("task text changed".to_owned());
    }
    let explicit = plan(&PlanInput {
        task,
        workflow: Some("feature"),
        definition: None,
        registry: &registry,
        concurrency: None,
        max_fix_iterations: None,
    })
    .map_err(|err| err.to_string())?;
    if explicit.workflow != "feature" || explicit.task != task {
        return Err("explicit workflow did not win".to_owned());
    }
    if explicit.id == first.id {
        return Err("explicit feature plan matched the task-type plan".to_owned());
    }
    Ok(format!("{} -> {}", first.selection.rule, first.workflow))
}

fn skill_cycle_case(case: &Case) -> Result<String, String> {
    let fs = MemFs::new();
    let root = Path::new("/proj");
    for (name, other) in [("a", "b"), ("b", "a")] {
        let dir = root.join(".agents/skills").join(name);
        fs.create_dir_all(&dir).map_err(|err| err.to_string())?;
        let body = format!(
            "---\nname: {name}\ndescription: Cycle skill {name}.\nversion: \"0.1.0\"\nrequires: [{other}]\neval_cases:\n  - task: use {name}\n---\n## When to use\nFixture.\n## Inputs\nNone.\n## Procedure\n1. Nothing.\n## Outputs\nNone.\n## Must not\nNothing.\n## Validation\nFixture.\n"
        );
        fs.write(&dir.join("SKILL.md"), body.as_bytes())
            .map_err(|err| err.to_string())?;
    }
    let discovery =
        SkillLoader::for_project(root, Arc::new(fs), None, builtin::location()).discover();
    let definition =
        WorkflowDefinition::named("cycle-skill", vec![StepSpec::new("work").with_skill("a")]);
    let err = plan(&PlanInput {
        task: task_of(case),
        workflow: None,
        definition: Some(&definition),
        registry: &discovery.registry,
        concurrency: None,
        max_fix_iterations: None,
    })
    .map(|_| "planned".to_owned())
    .unwrap_or_else(|err| err.to_string());
    expect_contains(
        &err,
        case.expect_error.as_deref().unwrap_or("dependency cycle"),
    )?;
    Ok(err)
}

fn run_definition(
    case: &Case,
    definition: &WorkflowDefinition,
    runner: MockRunner,
    validations: &[bool],
) -> Result<agentos_workflow::RunRecord, String> {
    let registry = empty_registry();
    let planned = plan_of(case, definition, &registry)?;
    let root = tempfile::tempdir().map_err(|err| err.to_string())?;
    let mut runner = runner;
    let mut validator = if validations.is_empty() {
        ScriptedValidator::pass()
    } else {
        ScriptedValidator::pass().script("check", validations)
    };
    let context = empty_context();
    let mut session = Session {
        root: root.path(),
        runner: &mut runner,
        validator: &mut validator,
        context: &context,
    };
    let run = start(&mut session, &planned).map_err(|err| err.to_string())?;
    if !case.expect_calls.is_empty() && runner.calls != case.expect_calls {
        return Err(format!(
            "calls {:?} != {:?}",
            runner.calls, case.expect_calls
        ));
    }
    Ok(run)
}

fn plan_of(
    case: &Case,
    definition: &WorkflowDefinition,
    registry: &SkillRegistry,
) -> Result<agentos_workflow::Plan, String> {
    plan(&PlanInput {
        task: task_of(case),
        workflow: None,
        definition: Some(definition),
        registry,
        concurrency: case.concurrency,
        max_fix_iterations: case.max_fix_iterations,
    })
    .map_err(|err| err.to_string())
}

fn definition_from(case: &Case) -> Result<WorkflowDefinition, String> {
    if case.steps.is_empty() {
        return Err("case has no steps".to_owned());
    }
    let mut steps = Vec::new();
    for step in &case.steps {
        let mut spec = StepSpec::new(&step.id);
        if !step.depends_on.is_empty() {
            let deps: Vec<&str> = step.depends_on.iter().map(String::as_str).collect();
            spec = spec.depends_on(&deps);
        }
        spec.consumes.clone_from(&step.consumes);
        spec.outputs.clone_from(&step.outputs);
        if let Some(skill) = &step.skill {
            spec = spec.with_skill(skill);
        }
        if let Some(kind) = &step.validation {
            spec = spec.with_validation(kind, step.fix_loop);
        }
        if !step.permissions.is_empty() {
            let caps: Vec<&str> = step.permissions.iter().map(String::as_str).collect();
            spec = spec.with_permissions(&caps);
        }
        if let Some(reason) = &step.skip_reason {
            spec = spec.skip(reason);
        }
        steps.push(spec);
    }
    let mut definition = WorkflowDefinition::named(&case.id, steps);
    if let Some(concurrency) = case.concurrency {
        definition.concurrency = concurrency;
    }
    if let Some(limit) = case.max_fix_iterations {
        definition.max_fix_iterations = limit;
    }
    Ok(definition)
}

fn check_table(scope: Scope, listed: &[Vec<String>]) -> Result<(), String> {
    let mut legal = Vec::new();
    for pair in listed {
        if pair.len() != 2 {
            return Err(format!("transition pair {pair:?} must have two states"));
        }
        let from = parse_state(&pair[0])?;
        let to = parse_state(&pair[1])?;
        legal.push((from, to));
    }
    for from in RunState::ALL {
        for to in RunState::ALL {
            let expected = legal.contains(&(from, to));
            if allows(scope, from, to) != expected {
                return Err(format!(
                    "{scope:?} {from} -> {to} legal={} listed={expected}",
                    allows(scope, from, to)
                ));
            }
        }
    }
    Ok(())
}

fn parse_state(name: &str) -> Result<RunState, String> {
    RunState::ALL
        .into_iter()
        .find(|state| state.as_str() == name)
        .ok_or_else(|| format!("unknown state {name}"))
}

fn expect_state(state: &RunState, case: &Case) -> Result<(), String> {
    if let Some(expected) = &case.expect_state
        && state.as_str() != expected
    {
        return Err(format!("state {state} != {expected}"));
    }
    Ok(())
}

fn expect_contains(actual: &str, needle: &str) -> Result<(), String> {
    if needle.is_empty() || actual.contains(needle) {
        Ok(())
    } else {
        Err(format!("{actual:?} does not contain {needle:?}"))
    }
}

fn task_of(case: &Case) -> &str {
    if case.task.is_empty() {
        "task"
    } else {
        case.task.as_str()
    }
}

fn empty_registry() -> SkillRegistry {
    SkillLoader::new().discover().registry
}

fn builtin_registry() -> SkillRegistry {
    SkillLoader::new()
        .with_location(builtin::location())
        .discover()
        .registry
}

fn empty_context() -> EngineContext<MemoryView> {
    EngineContext {
        view: MemoryView::default(),
        config: ContextConfig {
            budget: None,
            cache_dir: None,
        },
    }
}

fn weaken_edit(kind: &str) -> Result<TestEdit, String> {
    let path = "tests/test_payment.py".to_owned();
    let before = "def test_amount():\n    assert amount > 0\n\ndef test_sign():\n    assert True\n";
    let after = match kind {
        "delete" => String::new(),
        "assertion" => {
            "def test_amount():\n    assert amount > 0\n\ndef test_sign():\n    assert True\n"
                .replace("    assert True\n", "")
        }
        "remove" => "def test_amount():\n    assert amount > 0\n".to_owned(),
        other => return Err(format!("unknown weaken mode {other}")),
    };
    Ok(TestEdit {
        path,
        before: before.to_owned(),
        after,
    })
}

fn node(id: &str, kind: NodeKind, name: &str) -> KnowledgeNode {
    KnowledgeNode {
        id: id.to_owned(),
        node_type: kind,
        name: name.to_owned(),
        scope: String::new(),
        summary: name.to_owned(),
        status: "inferred".to_owned(),
        provenance: Provenance {
            kind: ProvenanceKind::Inferred,
            source: "app/x.py".to_owned(),
            location: "L1-L2".to_owned(),
            extractor: "test".to_owned(),
            extractor_version: "1".to_owned(),
            method: "test".to_owned(),
        },
        confidence_millis: 700,
        version: 1,
        fingerprint: format!("fp-{id}"),
        source_refs: vec!["app/x.py".to_owned()],
        retrieval_metadata: Default::default(),
        payload: Default::default(),
        source_id: "app/x.py".to_owned(),
        parent: None,
    }
}

fn fragment(node_id: &str, content: &str) -> KnowledgeFragment {
    KnowledgeFragment {
        id: format!("{node_id}#0"),
        node_id: node_id.to_owned(),
        source_ref: "app/x.py".to_owned(),
        location: "L1-L4".to_owned(),
        content: content.to_owned(),
        fingerprint: format!("frag-{node_id}"),
        token_estimate: agentos_knowledge::token_estimate(content),
        kind: "code".to_owned(),
    }
}
