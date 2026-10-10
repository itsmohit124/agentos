//! Scheduler. Ready steps are admitted in id order, up to the concurrency limit.
//!
//! The frontier is the full ready set. Admission is a prefix of that set. A join
//! stays out of the frontier until every declared dependency has passed.

use std::collections::BTreeMap;
use std::path::Path;

use agentos_permissions::{Capability, PermissionSet, PermissionSubject, check};

use crate::context::{self, ContextSource};
use crate::error::WorkflowError;
use crate::plan::Plan;
use crate::run::{self, RunRecord};
use crate::runner::{AgentRunner, RunMode, StepRequest};
use crate::state::RunState;
use crate::store;
use crate::validation::{self, PermissionCheck, ValidationRequest, Validator};
use crate::weaken;

/// Borrowed pieces a session needs.
pub struct Session<'a> {
    /// Project root. Runs are stored under `.agentos/runs/`.
    pub root: &'a Path,
    /// Step executor.
    pub runner: &'a mut dyn AgentRunner,
    /// Phase 5 validation.
    pub validator: &'a mut dyn Validator,
    /// Phase 4 context.
    pub context: &'a dyn ContextSource,
}

/// Starts a new run for `plan` and drives it until it stops.
pub fn start(session: &mut Session<'_>, plan: &Plan) -> Result<RunRecord, WorkflowError> {
    store::save_plan(session.root, plan)?;
    let n = store::runs_for_plan(session.root, &plan.id)? + 1;
    let mut run = RunRecord::from_plan(plan, format!("{}-{n}", plan.id));
    store::save_run(session.root, &run)?;
    drive(session, &mut run)?;
    Ok(run)
}

/// Continues a run that is `planned`, `ready`, or `running`.
pub fn resume(session: &mut Session<'_>, run_id: &str) -> Result<RunRecord, WorkflowError> {
    let mut run = store::load_run(session.root, run_id)?;
    if !matches!(
        run.state,
        RunState::Planned | RunState::Ready | RunState::Running
    ) {
        return Err(WorkflowError::NotResumable {
            id: run.id.clone(),
            state: run.state.as_str().to_owned(),
        });
    }
    drive(session, &mut run)?;
    Ok(run)
}

/// Reads a run.
pub fn status(root: &Path, run_id: &str) -> Result<RunRecord, WorkflowError> {
    store::load_run(root, run_id)
}

/// Aborts a run that is not already terminal.
pub fn abort(root: &Path, run_id: &str) -> Result<RunRecord, WorkflowError> {
    let mut run = store::load_run(root, run_id)?;
    run.transition_run(RunState::Aborted)?;
    let pending: Vec<String> = run
        .steps
        .iter()
        .filter(|step| {
            matches!(
                step.state,
                RunState::Planned | RunState::Ready | RunState::Running
            )
        })
        .map(|step| step.id.clone())
        .collect();
    for id in pending {
        run.transition_step(&id, RunState::Aborted)?;
    }
    run.waiting = false;
    run.interrupted = false;
    store::save_run(root, &run)?;
    Ok(run)
}

fn drive(session: &mut Session<'_>, run: &mut RunRecord) -> Result<(), WorkflowError> {
    if run.concurrency == 0 {
        return Err(WorkflowError::UnboundedConcurrency);
    }
    session.runner.begin_session();
    run.interrupted = false;
    run.waiting = false;
    match run.state {
        RunState::Planned => run.transition_run(RunState::Ready)?,
        RunState::Running => run::normalize_for_resume(run)?,
        RunState::Ready => {}
        _ => {
            return Err(WorkflowError::NotResumable {
                id: run.id.clone(),
                state: run.state.as_str().to_owned(),
            });
        }
    }
    store::save_run(session.root, run)?;

    loop {
        let frontier = mark_ready(run)?;
        if frontier.is_empty() {
            finish(run)?;
            store::save_run(session.root, run)?;
            return Ok(());
        }
        run.frontiers.push(frontier.clone());
        let limit = usize::try_from(run.concurrency).unwrap_or(1);
        let admit: Vec<String> = frontier.into_iter().take(limit).collect();
        run.waves.push(admit.clone());
        if run.state == RunState::Ready {
            run.transition_run(RunState::Running)?;
        }
        for id in admit {
            let control = run_step(session, run, &id)?;
            store::save_run(session.root, run)?;
            if control == Control::Stop {
                return Ok(());
            }
            if session.runner.suspended() {
                run.interrupted = true;
                store::save_run(session.root, run)?;
                return Ok(());
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Control {
    Continue,
    Stop,
}

fn mark_ready(run: &mut RunRecord) -> Result<Vec<String>, WorkflowError> {
    let ids = frontier_ids(run);
    for id in &ids {
        if step_state(run, id)? == RunState::Planned {
            run.transition_step(id, RunState::Ready)?;
        }
    }
    Ok(ids)
}

fn frontier_ids(run: &RunRecord) -> Vec<String> {
    let mut ids: Vec<String> = run
        .steps
        .iter()
        .filter(|step| {
            matches!(step.state, RunState::Planned | RunState::Ready)
                && step.depends_on.iter().all(|dep| {
                    run.steps
                        .iter()
                        .any(|other| other.id == *dep && other.state == RunState::Passed)
                })
        })
        .map(|step| step.id.clone())
        .collect();
    ids.sort();
    ids
}

fn finish(run: &mut RunRecord) -> Result<(), WorkflowError> {
    if run.steps.iter().any(|step| step.state == RunState::Failed) {
        if run.state != RunState::Failed {
            run.transition_run(RunState::Failed)?;
        }
        return Ok(());
    }
    if run.steps.iter().any(|step| step.state == RunState::Blocked) {
        if run.state != RunState::Blocked {
            run.transition_run(RunState::Blocked)?;
        }
        return Ok(());
    }
    if run.steps.iter().all(|step| step.state == RunState::Passed) {
        if run.state == RunState::Running {
            run.transition_run(RunState::Passed)?;
        }
        if run.state == RunState::Passed {
            run.transition_run(RunState::Completed)?;
        }
    }
    Ok(())
}

fn run_step(
    session: &mut Session<'_>,
    run: &mut RunRecord,
    id: &str,
) -> Result<Control, WorkflowError> {
    if !permit(run, id)? {
        let reason = step_block_reason(run, id);
        run.failure_reason = Some(reason);
        if run.state != RunState::Blocked {
            run.transition_run(RunState::Blocked)?;
        }
        return Ok(Control::Stop);
    }
    attach_context(session, run, id)?;
    if session.runner.needs_input(id) {
        let pending = request_for(run, id, RunMode::Execute)?;
        let instructions = pending.instructions.clone();
        session.runner.observe(&pending);
        run.pending_step = Some(id.to_owned());
        run.pending_instructions = Some(instructions);
        run.waiting = true;
        if run.state == RunState::Running {
            run.transition_run(RunState::Ready)?;
        }
        return Ok(Control::Stop);
    }
    run.transition_step(id, RunState::Running)?;
    if should_execute(run, id)? {
        let result = session.runner.run(&request(run, id, RunMode::Execute)?)?;
        run.executed
            .push(format!("{id}:{}", RunMode::Execute.as_str()));
        if result.failed {
            return fail_step(run, id, result.message);
        }
        store_outputs(run, id, result.outputs)?;
    }
    if has_validation(run, id)? {
        match validate_loop(session, run, id)? {
            Control::Stop => return Ok(Control::Stop),
            Control::Continue => {}
        }
    }
    if step_state(run, id)? == RunState::Running {
        run.transition_step(id, RunState::Passed)?;
    }
    Ok(Control::Continue)
}

fn permit(run: &mut RunRecord, id: &str) -> Result<bool, WorkflowError> {
    let index = index_of(run, id)?;
    let granted = parse_caps(&run.steps[index].granted)?;
    let required = parse_caps(&run.steps[index].required)?;
    let subject = Subject {
        label: format!("step '{id}'"),
        permissions: granted.into_iter().collect(),
    };
    let mut denied = None;
    let mut checks = Vec::new();
    for cap in required {
        let decision = check(&subject, cap);
        checks.push(PermissionCheck {
            capability: cap.as_str().to_owned(),
            verdict: if decision.is_allowed() {
                "allow".to_owned()
            } else {
                "deny".to_owned()
            },
            reason: decision.reason.clone(),
        });
        if !decision.is_allowed() && denied.is_none() {
            denied = Some(decision.reason);
        }
    }
    let index = index_of(run, id)?;
    run.steps[index].permission_checks = checks;
    if let Some(reason) = denied {
        run.steps[index].block_reason = Some(reason);
        run.transition_step(id, RunState::Blocked)?;
        return Ok(false);
    }
    Ok(true)
}

fn attach_context(
    session: &Session<'_>,
    run: &mut RunRecord,
    id: &str,
) -> Result<(), WorkflowError> {
    let (summary, dependency_outputs) = {
        let index = index_of(run, id)?;
        let step = &run.steps[index];
        (step.summary.clone(), dependency_outputs(run, step))
    };
    let task = context::step_task(&run.task, id, &summary);
    let pack = session.context.build_pack(&task)?;
    let mut fragment_ids: Vec<String> = pack
        .selected_fragments
        .iter()
        .map(|fragment| fragment.id.clone())
        .collect();
    fragment_ids.sort();
    let assembled = context::assemble(pack, dependency_outputs)?;
    let index = index_of(run, id)?;
    let step = &mut run.steps[index];
    step.context_task = Some(assembled.pack.task.clone());
    step.context_pack_id = Some(assembled.id.clone());
    step.selected_fragment_ids = fragment_ids;
    step.dependency_outputs = assembled.dependency_outputs.clone();
    step.context_pack = Some(assembled.pack);
    Ok(())
}

fn validate_loop(
    session: &mut Session<'_>,
    run: &mut RunRecord,
    id: &str,
) -> Result<Control, WorkflowError> {
    let (kind, fix_loop, limit) = {
        let index = index_of(run, id)?;
        let step = &run.steps[index];
        let kind = step
            .validation
            .clone()
            .ok_or_else(|| WorkflowError::Definition(format!("step {id} has no validation")))?;
        let limit = if step.fix_loop {
            run.max_fix_iterations
        } else {
            1
        };
        (kind, step.fix_loop, limit)
    };
    if limit == 0 {
        return fail_step(
            run,
            id,
            format!("validation failed for {id} after 0 attempts"),
        );
    }
    for attempt in 1..=limit {
        let request = validation_request(run, id, &kind, attempt)?;
        let result = session.validator.validate(&request)?;
        let failed = validation::rejected(&result);
        let summary = validation::feedback_text(&result);
        {
            let index = index_of(run, id)?;
            run.steps[index].attempts = attempt;
            run.steps[index].validations.push(result);
        }
        if !failed {
            return Ok(Control::Continue);
        }
        if !fix_loop || attempt == limit {
            return fail_step(
                run,
                id,
                format!("validation failed for {id} after {attempt} attempts: {summary}"),
            );
        }
        let fix = session.runner.run(&request_for(run, id, RunMode::Fix)?)?;
        run.executed.push(format!("{id}:{}", RunMode::Fix.as_str()));
        if fix.failed {
            return fail_step(run, id, fix.message);
        }
        if let Some(finding) = weaken::detect(&fix.edits) {
            return fail_step(run, id, format!("test weakening: {}", finding.detail));
        }
    }
    fail_step(
        run,
        id,
        format!("validation failed for {id} after {limit} attempts"),
    )
}

fn fail_step(run: &mut RunRecord, id: &str, reason: String) -> Result<Control, WorkflowError> {
    let index = index_of(run, id)?;
    run.steps[index].failure_reason = Some(reason.clone());
    if run.steps[index].state == RunState::Running {
        run.transition_step(id, RunState::Failed)?;
    }
    run.failure_reason = Some(reason);
    if run.state != RunState::Failed {
        run.transition_run(RunState::Failed)?;
    }
    Ok(Control::Stop)
}

fn validation_request(
    run: &RunRecord,
    id: &str,
    kind: &str,
    attempt: u32,
) -> Result<ValidationRequest, WorkflowError> {
    let index = index_of(run, id)?;
    let step = &run.steps[index];
    Ok(ValidationRequest {
        step_id: id.to_owned(),
        attempt,
        kind: kind.to_owned(),
        task_id: run.task.clone(),
        context_pack_id: step.context_pack_id.clone().unwrap_or_default(),
        granted: step.granted.clone(),
        target: session_target(run),
    })
}

fn session_target(run: &RunRecord) -> String {
    run.plan_id.clone()
}

fn request(run: &RunRecord, id: &str, mode: RunMode) -> Result<StepRequest, WorkflowError> {
    request_for(run, id, mode)
}

fn request_for(run: &RunRecord, id: &str, mode: RunMode) -> Result<StepRequest, WorkflowError> {
    let index = index_of(run, id)?;
    let step = &run.steps[index];
    Ok(StepRequest {
        step_id: id.to_owned(),
        mode,
        task: run.task.clone(),
        instructions: instructions(run, id),
        outputs: step.outputs.clone(),
        context_pack_id: step.context_pack_id.clone().unwrap_or_default(),
        context_task: step.context_task.clone().unwrap_or_default(),
        dependency_outputs: step.dependency_outputs.clone(),
        selected_fragment_ids: step.selected_fragment_ids.clone(),
    })
}

fn instructions(run: &RunRecord, id: &str) -> String {
    let step = run.steps.iter().find(|step| step.id == id);
    let summary = step.map(|step| step.summary.as_str()).unwrap_or(id);
    let skill = step
        .and_then(|step| step.skill.as_deref())
        .unwrap_or("(none)");
    format!(
        "Task: {task}\nStep: {id}\nSummary: {summary}\nSkill: {skill}\nCarry out this step. Do not change the task. Do not weaken tests.\n",
        task = run.task
    )
}

fn dependency_outputs(run: &RunRecord, step: &crate::run::StepRun) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for dep_id in &step.depends_on {
        let Some(dep) = run.steps.iter().find(|other| &other.id == dep_id) else {
            continue;
        };
        for (name, value) in &dep.outputs_produced {
            if step.consumes.is_empty() || step.consumes.iter().any(|item| item == name) {
                out.insert(format!("{dep_id}.{name}"), value.clone());
            }
        }
    }
    out
}

fn store_outputs(
    run: &mut RunRecord,
    id: &str,
    outputs: BTreeMap<String, String>,
) -> Result<(), WorkflowError> {
    let index = index_of(run, id)?;
    run.steps[index].outputs_produced = outputs;
    Ok(())
}

fn should_execute(run: &RunRecord, id: &str) -> Result<bool, WorkflowError> {
    let index = index_of(run, id)?;
    let step = &run.steps[index];
    Ok(step.skill.is_some() || step.validation.is_none())
}

fn has_validation(run: &RunRecord, id: &str) -> Result<bool, WorkflowError> {
    let index = index_of(run, id)?;
    Ok(run.steps[index].validation.is_some())
}

fn step_state(run: &RunRecord, id: &str) -> Result<RunState, WorkflowError> {
    let index = index_of(run, id)?;
    Ok(run.steps[index].state)
}

fn step_block_reason(run: &RunRecord, id: &str) -> String {
    run.steps
        .iter()
        .find(|step| step.id == id)
        .and_then(|step| step.block_reason.clone())
        .unwrap_or_else(|| format!("permission denied for step {id}"))
}

fn index_of(run: &RunRecord, id: &str) -> Result<usize, WorkflowError> {
    run.steps
        .iter()
        .position(|step| step.id == id)
        .ok_or_else(|| WorkflowError::Definition(format!("unknown step {id:?}")))
}

fn parse_caps(names: &[String]) -> Result<Vec<Capability>, WorkflowError> {
    let mut caps = Vec::new();
    for name in names {
        caps.push(
            name.parse()
                .map_err(|err: agentos_permissions::UnknownCapability| {
                    WorkflowError::Definition(err.to_string())
                })?,
        );
    }
    Ok(caps)
}

struct Subject {
    label: String,
    permissions: PermissionSet,
}

impl PermissionSubject for Subject {
    fn subject(&self) -> String {
        self.label.clone()
    }

    fn permissions(&self) -> &PermissionSet {
        &self.permissions
    }
}
