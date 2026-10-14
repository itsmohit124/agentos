//! Contract tests for planning, scheduling, resume, and the fix loop.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use agentos_context::{ContextConfig, MemoryView};
use agentos_core::fs::{FileSystem, MemFs};
use agentos_knowledge::{KnowledgeFragment, KnowledgeNode, NodeKind, Provenance, ProvenanceKind};
use agentos_skills::{SkillLoader, SkillRegistry, builtin};

use crate::definition::{StepSpec, WorkflowDefinition};
use crate::execute::Session;
use crate::plan::PlanInput;
use crate::run::RunRecord;
use crate::runner::{ManualRunner, MockReaction, MockRunner, RunnerResult};
use crate::state::{RunState, Scope, allows};
use crate::weaken::TestEdit;
use crate::{
    EngineContext, Plan, ScriptedValidator, WorkflowError, detect, load, load_plan, load_run,
    normalize_for_resume, plan, recover_index, resume, start, status,
};

fn empty_registry() -> SkillRegistry {
    SkillLoader::new().discover().registry
}

fn builtin_registry() -> SkillRegistry {
    SkillLoader::new()
        .with_location(builtin::location())
        .discover()
        .registry
}

fn context_of(view: MemoryView) -> EngineContext<MemoryView> {
    EngineContext {
        view,
        config: ContextConfig {
            budget: None,
            cache_dir: None,
        },
    }
}

fn plan_def(task: &str, definition: &WorkflowDefinition, registry: &SkillRegistry) -> Plan {
    plan(&PlanInput {
        task,
        workflow: None,
        definition: Some(definition),
        registry,
        concurrency: None,
        max_fix_iterations: None,
    })
    .unwrap_or_else(|err| panic!("plan failed: {err}"))
}

fn run_plan(
    root: &Path,
    planned: &Plan,
    runner: &mut dyn crate::AgentRunner,
    validator: &mut ScriptedValidator,
    view: MemoryView,
) -> RunRecord {
    let context = context_of(view);
    let mut session = Session {
        root,
        runner,
        validator,
        context: &context,
    };
    start(&mut session, planned).unwrap_or_else(|err| panic!("start failed: {err}"))
}

fn fanout() -> WorkflowDefinition {
    WorkflowDefinition::named(
        "fanout",
        vec![
            StepSpec::new("a"),
            StepSpec::new("b").depends_on(&["a"]),
            StepSpec::new("c").depends_on(&["a"]),
            StepSpec::new("d").depends_on(&["a"]),
        ],
    )
}

fn join() -> WorkflowDefinition {
    WorkflowDefinition::named(
        "join",
        vec![
            StepSpec::new("a"),
            StepSpec::new("b").depends_on(&["a"]),
            StepSpec::new("c").depends_on(&["a"]),
            StepSpec::new("d").depends_on(&["b", "c"]),
        ],
    )
}

fn linear() -> WorkflowDefinition {
    WorkflowDefinition::named(
        "linear",
        vec![
            StepSpec::new("a"),
            StepSpec::new("b").depends_on(&["a"]),
            StepSpec::new("c").depends_on(&["b"]),
        ],
    )
}

fn ids(run: &RunRecord) -> Vec<&str> {
    run.steps
        .iter()
        .filter(|step| step.state == RunState::Passed)
        .map(|step| step.id.as_str())
        .collect()
}

#[test]
fn ready_frontier_admits_siblings_together() {
    let registry = empty_registry();
    let definition = fanout();
    let planned = plan_def("task", &definition, &registry);
    let root = tempfile::tempdir().unwrap();
    let mut runner = MockRunner::succeed();
    let mut validator = ScriptedValidator::pass();
    let run = run_plan(
        root.path(),
        &planned,
        &mut runner,
        &mut validator,
        MemoryView::default(),
    );
    assert_eq!(run.state, RunState::Completed);
    assert!(
        run.frontiers
            .iter()
            .any(|frontier| frontier == &vec!["b".to_owned(), "c".to_owned(), "d".to_owned()])
    );
    assert_eq!(
        run.waves,
        vec![
            vec!["a".to_owned()],
            vec!["b".to_owned(), "c".to_owned(), "d".to_owned()]
        ]
    );
}

#[test]
fn join_waits_for_every_dependency() {
    let registry = empty_registry();
    let definition = join();
    let planned = plan_def("task", &definition, &registry);
    let root = tempfile::tempdir().unwrap();
    let mut runner = MockRunner::succeed();
    let mut validator = ScriptedValidator::pass();
    let run = run_plan(
        root.path(),
        &planned,
        &mut runner,
        &mut validator,
        MemoryView::default(),
    );
    assert_eq!(
        run.waves,
        vec![
            vec!["a".to_owned()],
            vec!["b".to_owned(), "c".to_owned()],
            vec!["d".to_owned()]
        ]
    );
    let with_d = run
        .frontiers
        .iter()
        .find(|frontier| frontier.iter().any(|id| id == "d"))
        .unwrap();
    assert_eq!(with_d, &vec!["d".to_owned()]);
}

#[test]
fn concurrency_is_bounded_and_deterministic() {
    let registry = empty_registry();
    let mut definition = fanout();
    definition.concurrency = 2;
    let planned = plan_def("task", &definition, &registry);
    let root = tempfile::tempdir().unwrap();
    let mut runner = MockRunner::succeed();
    let mut validator = ScriptedValidator::pass();
    let first = run_plan(
        root.path(),
        &planned,
        &mut runner,
        &mut validator,
        MemoryView::default(),
    );
    assert!(first.waves.iter().all(|wave| wave.len() <= 2));
    assert_eq!(
        first.waves,
        vec![
            vec!["a".to_owned()],
            vec!["b".to_owned(), "c".to_owned()],
            vec!["d".to_owned()]
        ]
    );
    assert!(
        first
            .frontiers
            .iter()
            .any(|frontier| frontier == &vec!["b".to_owned(), "c".to_owned(), "d".to_owned()])
    );
    let other = tempfile::tempdir().unwrap();
    let mut runner = MockRunner::succeed();
    let mut validator = ScriptedValidator::pass();
    let second = run_plan(
        other.path(),
        &planned,
        &mut runner,
        &mut validator,
        MemoryView::default(),
    );
    assert_eq!(first.waves, second.waves);
    assert_eq!(first.frontiers, second.frontiers);
}

#[test]
fn workflow_cycle_fails_with_the_path() {
    let registry = empty_registry();
    let definition = WorkflowDefinition::named(
        "cycle",
        vec![
            StepSpec::new("a").depends_on(&["b"]),
            StepSpec::new("b").depends_on(&["a"]),
        ],
    );
    let err = plan(&PlanInput {
        task: "task",
        workflow: None,
        definition: Some(&definition),
        registry: &registry,
        concurrency: None,
        max_fix_iterations: None,
    })
    .unwrap_err();
    assert_eq!(err.to_string(), "workflow cycle: a -> b -> a");
}

#[test]
fn missing_skill_fails_planning() {
    let registry = empty_registry();
    let definition = WorkflowDefinition::named(
        "missing",
        vec![StepSpec::new("work").with_skill("missing-skill")],
    );
    let err = plan(&PlanInput {
        task: "task",
        workflow: None,
        definition: Some(&definition),
        registry: &registry,
        concurrency: None,
        max_fix_iterations: None,
    })
    .unwrap_err();
    let message = err.to_string();
    assert!(message.contains("missing-skill"), "{message}");
    assert!(message.contains("unknown skill"), "{message}");
}

#[test]
fn skill_dependency_cycle_fails_planning() {
    let fs = MemFs::new();
    let root = Path::new("/proj");
    for (name, other) in [("a", "b"), ("b", "a")] {
        let dir = root.join(".agents/skills").join(name);
        fs.create_dir_all(&dir).unwrap();
        let body = format!(
            "---\nname: {name}\ndescription: Cycle skill {name}.\nversion: \"0.1.0\"\nrequires: [{other}]\neval_cases:\n  - task: use {name}\n---\n## When to use\nFixture.\n## Inputs\nNone.\n## Procedure\n1. Nothing.\n## Outputs\nNone.\n## Must not\nNothing.\n## Validation\nFixture.\n"
        );
        fs.write(&dir.join("SKILL.md"), body.as_bytes()).unwrap();
    }
    let discovery =
        SkillLoader::for_project(root, Arc::new(fs), None, builtin::location()).discover();
    assert!(
        discovery.registry.get("a").is_some(),
        "{:?}",
        discovery.diagnostics
    );
    let definition =
        WorkflowDefinition::named("cycle-skill", vec![StepSpec::new("work").with_skill("a")]);
    let err = plan(&PlanInput {
        task: "task",
        workflow: None,
        definition: Some(&definition),
        registry: &discovery.registry,
        concurrency: None,
        max_fix_iterations: None,
    })
    .unwrap_err();
    let message = err.to_string();
    assert!(message.contains("dependency cycle"), "{message}");
}

#[test]
fn permission_denial_does_not_call_the_runner() {
    let registry = empty_registry();
    let definition = WorkflowDefinition::named(
        "denied",
        vec![
            StepSpec::new("gate").with_validation("unit_test", false),
            StepSpec::new("after").depends_on(&["gate"]),
        ],
    );
    let planned = plan_def("task", &definition, &registry);
    let gate = planned.steps.iter().find(|step| step.id == "gate").unwrap();
    assert!(gate.required.iter().any(|cap| cap == "shell"));
    assert!(!gate.granted.iter().any(|cap| cap == "shell"));
    let root = tempfile::tempdir().unwrap();
    let mut runner = MockRunner::succeed();
    let mut validator = ScriptedValidator::pass();
    let run = run_plan(
        root.path(),
        &planned,
        &mut runner,
        &mut validator,
        MemoryView::default(),
    );
    assert_eq!(run.state, RunState::Blocked);
    assert!(runner.calls.is_empty(), "{:?}", runner.calls);
    assert!(validator.calls.is_empty(), "{:?}", validator.calls);
    let gate = run.steps.iter().find(|step| step.id == "gate").unwrap();
    assert_eq!(gate.state, RunState::Blocked);
    let reason = gate.block_reason.as_deref().unwrap_or("");
    assert!(reason.contains("does not declare shell"), "{reason}");
    assert!(
        run.steps
            .iter()
            .any(|step| step.id == "after" && step.state != RunState::Passed)
    );
}

#[test]
fn resume_executes_only_the_unfinished_step() {
    let registry = empty_registry();
    let definition = linear();
    let planned = plan_def("task", &definition, &registry);
    let root = tempfile::tempdir().unwrap();
    let mut runner = MockRunner::succeed().suspend_after("b");
    let mut validator = ScriptedValidator::pass();
    let context = context_of(MemoryView::default());
    let run = {
        let mut session = Session {
            root: root.path(),
            runner: &mut runner,
            validator: &mut validator,
            context: &context,
        };
        start(&mut session, &planned).unwrap()
    };
    assert!(run.interrupted);
    assert_eq!(ids(&run), vec!["a", "b"]);
    assert_eq!(
        runner.calls,
        vec!["a:execute".to_owned(), "b:execute".to_owned()]
    );
    let c = run.steps.iter().find(|step| step.id == "c").unwrap();
    assert_ne!(c.state, RunState::Passed);
    let run = {
        let mut session = Session {
            root: root.path(),
            runner: &mut runner,
            validator: &mut validator,
            context: &context,
        };
        resume(&mut session, &run.id).unwrap()
    };
    assert_eq!(run.state, RunState::Completed);
    assert_eq!(
        runner.calls,
        vec![
            "a:execute".to_owned(),
            "b:execute".to_owned(),
            "c:execute".to_owned()
        ]
    );
    assert_eq!(
        run.executed
            .iter()
            .filter(|call| call.starts_with("a:"))
            .count(),
        1
    );
    assert_eq!(
        run.executed
            .iter()
            .filter(|call| call.starts_with("b:"))
            .count(),
        1
    );
    assert_eq!(
        run.executed
            .iter()
            .filter(|call| call.starts_with("c:"))
            .count(),
        1
    );
}

#[test]
fn abort_is_terminal_and_keeps_passed_steps() {
    let registry = empty_registry();
    let definition = linear();
    let planned = plan_def("task", &definition, &registry);
    let root = tempfile::tempdir().unwrap();
    let mut runner = MockRunner::succeed().suspend_after("a");
    let mut validator = ScriptedValidator::pass();
    let context = context_of(MemoryView::default());
    let run = {
        let mut session = Session {
            root: root.path(),
            runner: &mut runner,
            validator: &mut validator,
            context: &context,
        };
        start(&mut session, &planned).unwrap()
    };
    let aborted = crate::abort(root.path(), &run.id).unwrap();
    assert_eq!(aborted.state, RunState::Aborted);
    assert_eq!(
        aborted
            .steps
            .iter()
            .find(|step| step.id == "a")
            .unwrap()
            .state,
        RunState::Passed
    );
    assert_eq!(
        aborted
            .steps
            .iter()
            .find(|step| step.id == "b")
            .unwrap()
            .state,
        RunState::Aborted
    );
    let err = {
        let mut session = Session {
            root: root.path(),
            runner: &mut runner,
            validator: &mut validator,
            context: &context,
        };
        resume(&mut session, &run.id).unwrap_err()
    };
    assert!(matches!(err, WorkflowError::NotResumable { .. }), "{err}");
    let err = crate::abort(root.path(), &run.id).unwrap_err();
    assert!(
        matches!(err, WorkflowError::IllegalTransition { .. }),
        "{err}"
    );
}

#[test]
fn state_machine_matches_the_legal_table() {
    let run_edges = [
        (RunState::Planned, RunState::Ready),
        (RunState::Planned, RunState::Aborted),
        (RunState::Ready, RunState::Running),
        (RunState::Ready, RunState::Blocked),
        (RunState::Ready, RunState::Aborted),
        (RunState::Running, RunState::Ready),
        (RunState::Running, RunState::Passed),
        (RunState::Running, RunState::Failed),
        (RunState::Running, RunState::Blocked),
        (RunState::Running, RunState::Aborted),
        (RunState::Passed, RunState::Completed),
    ];
    let step_edges = [
        (RunState::Planned, RunState::Ready),
        (RunState::Planned, RunState::Aborted),
        (RunState::Ready, RunState::Running),
        (RunState::Ready, RunState::Blocked),
        (RunState::Ready, RunState::Aborted),
        (RunState::Running, RunState::Passed),
        (RunState::Running, RunState::Failed),
        (RunState::Running, RunState::Aborted),
        (RunState::Running, RunState::Ready),
    ];
    for from in RunState::ALL {
        for to in RunState::ALL {
            assert_eq!(
                allows(Scope::Run, from, to),
                run_edges.contains(&(from, to)),
                "run {from} -> {to}"
            );
            assert_eq!(
                allows(Scope::Step, from, to),
                step_edges.contains(&(from, to)),
                "step {from} -> {to}"
            );
        }
    }
    let registry = empty_registry();
    let definition = WorkflowDefinition::named("one", vec![StepSpec::new("a")]);
    let planned = plan_def("task", &definition, &registry);
    let mut run = RunRecord::from_plan(&planned, "run-1".to_owned());
    let err = run.transition_run(RunState::Completed).unwrap_err();
    assert!(matches!(err, WorkflowError::IllegalTransition { .. }));
    assert_eq!(run.state, RunState::Planned);
    assert!(run.transitions.is_empty());
    run.transition_run(RunState::Ready).unwrap();
    run.transition_step("a", RunState::Ready).unwrap();
    run.transition_step("a", RunState::Running).unwrap();
    normalize_for_resume(&mut run).unwrap();
    assert_eq!(
        run.steps[0].state,
        RunState::Ready,
        "an interrupted running step returns to ready"
    );
}

#[test]
fn each_step_gets_its_own_pack_and_declared_outputs() {
    let registry = builtin_registry();
    let feature = load("feature").unwrap();
    assert_eq!(feature.id, "feature");
    let planned = plan(&PlanInput {
        task: "Add retry support to payment webhook processing",
        workflow: Some("feature"),
        definition: None,
        registry: &registry,
        concurrency: None,
        max_fix_iterations: None,
    })
    .unwrap();
    assert_eq!(
        planned.task,
        "Add retry support to payment webhook processing"
    );
    let root = tempfile::tempdir().unwrap();
    let mut runner = MockRunner::succeed();
    let mut validator = ScriptedValidator::pass();
    let run = run_plan(
        root.path(),
        &planned,
        &mut runner,
        &mut validator,
        MemoryView::default(),
    );
    assert_eq!(run.state, RunState::Completed, "{:?}", run.failure_reason);
    let mut pack_ids = Vec::new();
    for step in &run.steps {
        let task = step.context_task.as_deref().unwrap_or("");
        assert!(task.contains(&format!("Step: {}", step.id)), "{task}");
        assert!(task.starts_with("Add retry support to payment webhook processing"));
        pack_ids.push(step.context_pack_id.clone().unwrap());
        assert!(step.context_pack.is_some());
    }
    pack_ids.sort();
    pack_ids.dedup();
    assert_eq!(pack_ids.len(), run.steps.len());
    let domain = run
        .steps
        .iter()
        .find(|step| step.id == "domain_design")
        .unwrap();
    assert_eq!(
        domain
            .dependency_outputs
            .get("research.findings")
            .map(String::as_str),
        Some("findings:ok")
    );
    let implementation = run
        .steps
        .iter()
        .find(|step| step.id == "implementation")
        .unwrap();
    assert!(
        implementation
            .dependency_outputs
            .contains_key("interface.interface")
    );
    assert!(
        !implementation
            .dependency_outputs
            .contains_key("interface.contract_tests")
    );
    assert!(
        !implementation
            .dependency_outputs
            .contains_key("research.findings")
    );
    let tdd = planned
        .skills
        .iter()
        .find(|skill| skill.step_id == "tdd" && skill.name == "interface-design")
        .unwrap();
    assert_eq!(
        tdd.reason,
        "required by step tdd because tdd requires interface-design"
    );
}

#[test]
fn unrelated_repository_context_is_not_included() {
    let registry = empty_registry();
    let definition = WorkflowDefinition::named("context", vec![StepSpec::new("work")]);
    let planned = plan_def(
        "Add retry support to payment webhook processing",
        &definition,
        &registry,
    );
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
    let root = tempfile::tempdir().unwrap();
    let mut runner = MockRunner::succeed();
    let mut validator = ScriptedValidator::pass();
    let run = run_plan(root.path(), &planned, &mut runner, &mut validator, view);
    let step = &run.steps[0];
    assert!(
        step.selected_fragment_ids
            .iter()
            .any(|id| id.contains("Webhook"))
    );
    assert!(
        step.selected_fragment_ids
            .iter()
            .all(|id| !id.contains("Recipe")),
        "{:?}",
        step.selected_fragment_ids
    );
    let pack = step.context_pack.as_ref().unwrap();
    assert!(
        pack.selected_nodes
            .iter()
            .all(|item| item.id != "software.app.cooking.function.Recipe")
    );
}

#[test]
fn fix_loop_succeeds_on_the_third_attempt() {
    let registry = empty_registry();
    let mut definition = WorkflowDefinition::named(
        "fix-ok",
        vec![
            StepSpec::new("check")
                .with_validation("unit_test", true)
                .with_permissions(&["shell"]),
        ],
    );
    definition.max_fix_iterations = 3;
    let planned = plan_def("task", &definition, &registry);
    let root = tempfile::tempdir().unwrap();
    let mut runner = MockRunner::succeed();
    let mut validator = ScriptedValidator::pass().script("check", &[false, false, true]);
    let run = run_plan(
        root.path(),
        &planned,
        &mut runner,
        &mut validator,
        MemoryView::default(),
    );
    assert_eq!(run.state, RunState::Completed, "{:?}", run.failure_reason);
    assert_eq!(validator.calls, vec!["check", "check", "check"]);
    assert_eq!(runner.calls, vec!["check:fix", "check:fix"]);
    assert!(run.steps[0].validations.last().unwrap().passed);
    assert_eq!(run.steps[0].validations.last().unwrap().feedback.len(), 0);
    assert!(!run.steps[0].validations[0].feedback.is_empty());
}

#[test]
fn fix_loop_exhaustion_fails_the_run() {
    let registry = empty_registry();
    let mut definition = WorkflowDefinition::named(
        "fix-fail",
        vec![
            StepSpec::new("check")
                .with_validation("unit_test", true)
                .with_permissions(&["shell"]),
        ],
    );
    definition.max_fix_iterations = 2;
    let planned = plan_def("task", &definition, &registry);
    let root = tempfile::tempdir().unwrap();
    let mut runner = MockRunner::succeed();
    let mut validator = ScriptedValidator::pass().script("check", &[false, false, true]);
    let run = run_plan(
        root.path(),
        &planned,
        &mut runner,
        &mut validator,
        MemoryView::default(),
    );
    assert_eq!(run.state, RunState::Failed);
    assert_eq!(validator.calls, vec!["check", "check"]);
    assert_eq!(runner.calls, vec!["check:fix"]);
    let reason = run.failure_reason.unwrap_or_default();
    assert!(reason.contains("after 2 attempts"), "{reason}");
    assert!(reason.contains("validation failed"), "{reason}");
}

#[test]
fn deleting_a_test_fails_the_fix_loop() {
    assert_weakening(
        TestEdit {
            path: "tests/test_payment.py".to_owned(),
            before: "def test_amount():\n    assert amount > 0\n".to_owned(),
            after: String::new(),
        },
        "deleted",
    );
}

#[test]
fn weakening_an_assertion_fails_the_fix_loop() {
    assert_weakening(
        TestEdit {
            path: "tests/test_payment.py".to_owned(),
            before: "def test_amount():\n    assert amount > 0\n    assert amount < 10\n"
                .to_owned(),
            after: "def test_amount():\n    assert amount > 0\n".to_owned(),
        },
        "assertion",
    );
}

#[test]
fn removing_a_test_fails_the_fix_loop() {
    assert_weakening(
        TestEdit {
            path: "tests/test_payment.py".to_owned(),
            before:
                "def test_amount():\n    assert amount > 0\n\ndef test_sign():\n    assert True\n"
                    .to_owned(),
            after: "def test_amount():\n    assert amount > 0\n".to_owned(),
        },
        "removed_test",
    );
}

#[test]
fn a_production_edit_is_not_test_weakening() {
    let finding = detect(&[TestEdit {
        path: "src/payment.py".to_owned(),
        before: "def charge():\n    assert False\n".to_owned(),
        after: "def charge():\n    return 1\n".to_owned(),
    }]);
    assert!(finding.is_none());
}

fn assert_weakening(edit: TestEdit, kind: &str) {
    let registry = empty_registry();
    let definition = WorkflowDefinition::named(
        "weaken",
        vec![
            StepSpec::new("check")
                .with_validation("unit_test", true)
                .with_permissions(&["shell"]),
        ],
    );
    let planned = plan_def("task", &definition, &registry);
    let root = tempfile::tempdir().unwrap();
    let mut runner = MockRunner::succeed().script("check", vec![MockReaction::edits(vec![edit])]);
    let mut validator = ScriptedValidator::pass().script("check", &[false, true]);
    let run = run_plan(
        root.path(),
        &planned,
        &mut runner,
        &mut validator,
        MemoryView::default(),
    );
    assert_eq!(run.state, RunState::Failed);
    assert_eq!(validator.calls, vec!["check".to_owned()]);
    let reason = run.failure_reason.unwrap_or_default();
    assert!(reason.contains("test weakening"), "{reason}");
    assert!(
        reason.contains(kind)
            || reason.contains("deleted")
            || reason.contains("removed")
            || reason.contains("weakened"),
        "{reason}"
    );
    if kind == "deleted" {
        assert!(reason.contains("deleted"), "{reason}");
    }
    if kind == "assertion" {
        assert!(reason.contains("weakened assertion"), "{reason}");
    }
    if kind == "removed_test" {
        assert!(reason.contains("removed test"), "{reason}");
    }
}

#[test]
fn shipped_workflows_skip_inapplicable_steps_and_complete() {
    let registry = builtin_registry();
    let root = tempfile::tempdir().unwrap();
    for (name, skipped) in [
        (
            "feature",
            vec![(
                "integration_test",
                "workflow does not require integration validation for this task type",
            )],
        ),
        (
            "bugfix",
            vec![
                (
                    "domain_design",
                    "workflow does not require domain design for this task type",
                ),
                (
                    "interface",
                    "workflow does not require interface design for this task type",
                ),
                (
                    "integration_test",
                    "workflow does not require integration validation for this task type",
                ),
                (
                    "architecture_review",
                    "workflow does not require architecture review for this task type",
                ),
            ],
        ),
        (
            "refactor",
            vec![
                (
                    "research",
                    "workflow does not require research for this task type",
                ),
                (
                    "domain_design",
                    "workflow does not require domain design for this task type",
                ),
                (
                    "integration_test",
                    "workflow does not require integration validation for this task type",
                ),
            ],
        ),
    ] {
        let planned = plan(&PlanInput {
            task: "Add retry support to payment webhook processing",
            workflow: Some(name),
            definition: None,
            registry: &registry,
            concurrency: None,
            max_fix_iterations: None,
        })
        .unwrap();
        assert_eq!(planned.workflow, name);
        assert_eq!(
            planned.task,
            "Add retry support to payment webhook processing"
        );
        for (id, reason) in &skipped {
            let found = planned
                .skipped
                .iter()
                .find(|step| step.id == *id)
                .unwrap_or_else(|| panic!("{name} did not skip {id}"));
            assert_eq!(found.reason, *reason);
            assert!(planned.steps.iter().all(|step| step.id != *id));
        }
        let mut runner = MockRunner::succeed();
        let mut validator = ScriptedValidator::pass();
        let run = run_plan(
            root.path(),
            &planned,
            &mut runner,
            &mut validator,
            MemoryView::default(),
        );
        assert_eq!(
            run.state,
            RunState::Completed,
            "{name}: {:?}",
            run.failure_reason
        );
        assert!(run.steps.iter().all(|step| step.context_pack_id.is_some()));
        let stored = status(root.path(), &run.id).unwrap();
        assert_eq!(stored.state, RunState::Completed);
        assert!(
            root.path()
                .join(".agentos/runs")
                .join(&run.id)
                .join("run.json")
                .is_file()
        );
        let loaded = load_plan(root.path(), &planned.id).unwrap();
        assert_eq!(loaded, planned);
    }
}

#[test]
fn plan_is_deterministic_and_explicit_workflow_wins() {
    let registry = builtin_registry();
    let task = "Fix the failing webhook";
    let first = plan(&PlanInput {
        task,
        workflow: None,
        definition: None,
        registry: &registry,
        concurrency: None,
        max_fix_iterations: None,
    })
    .unwrap();
    let second = plan(&PlanInput {
        task,
        workflow: None,
        definition: None,
        registry: &registry,
        concurrency: None,
        max_fix_iterations: None,
    })
    .unwrap();
    assert_eq!(first, second);
    assert_eq!(first.task, task);
    assert_eq!(first.workflow, "bugfix");
    assert_eq!(first.selection.source, "task-type");
    let explicit = plan(&PlanInput {
        task,
        workflow: Some("feature"),
        definition: None,
        registry: &registry,
        concurrency: None,
        max_fix_iterations: None,
    })
    .unwrap();
    assert_eq!(explicit.task, task);
    assert_eq!(explicit.workflow, "feature");
    assert_eq!(explicit.selection.source, "explicit");
    assert_ne!(explicit.id, first.id);
}

#[test]
fn zero_concurrency_is_rejected() {
    let registry = empty_registry();
    let definition = WorkflowDefinition::named("one", vec![StepSpec::new("a")]);
    let err = plan(&PlanInput {
        task: "task",
        workflow: None,
        definition: Some(&definition),
        registry: &registry,
        concurrency: Some(0),
        max_fix_iterations: None,
    })
    .unwrap_err();
    assert!(matches!(err, WorkflowError::UnboundedConcurrency));
}

#[test]
fn manual_runner_emits_instructions_then_records_the_result() {
    let registry = empty_registry();
    let definition = WorkflowDefinition::named("manual", vec![StepSpec::new("only")]);
    let planned = plan_def("Keep this task", &definition, &registry);
    let root = tempfile::tempdir().unwrap();
    let mut runner = ManualRunner::new();
    let mut validator = ScriptedValidator::pass();
    let context = context_of(MemoryView::default());
    let waiting = {
        let mut session = Session {
            root: root.path(),
            runner: &mut runner,
            validator: &mut validator,
            context: &context,
        };
        start(&mut session, &planned).unwrap()
    };
    assert!(waiting.waiting);
    assert_eq!(waiting.state, RunState::Ready);
    assert_eq!(waiting.pending_step.as_deref(), Some("only"));
    let instructions = waiting.pending_instructions.unwrap();
    assert!(instructions.contains("Keep this task"));
    assert!(instructions.contains("Step: only"));
    assert_eq!(runner.emissions.len(), 1);
    assert!(runner.emissions[0].context_task.contains("Step: only"));
    assert!(waiting.executed.is_empty());
    runner.push("only", RunnerResult::success("recorded"));
    let done = {
        let mut session = Session {
            root: root.path(),
            runner: &mut runner,
            validator: &mut validator,
            context: &context,
        };
        resume(&mut session, &waiting.id).unwrap()
    };
    assert_eq!(done.state, RunState::Completed);
    assert_eq!(done.executed, vec!["only:execute".to_owned()]);
    assert_eq!(runner.emissions.len(), 2);
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
        retrieval_metadata: BTreeMap::new(),
        payload: BTreeMap::new(),
        source_id: "app/x.py".to_owned(),
        parent: None,
    }
}

#[test]
fn interrupted_run_resumes_and_a_corrupt_index_can_be_rebuilt() {
    let registry = empty_registry();
    let definition = linear();
    let planned = plan_def("task", &definition, &registry);
    let root = tempfile::tempdir().unwrap();
    let mut runner = MockRunner::succeed().suspend_after("b");
    let mut validator = ScriptedValidator::pass();
    let context = context_of(MemoryView::default());
    let run = {
        let mut session = Session {
            root: root.path(),
            runner: &mut runner,
            validator: &mut validator,
            context: &context,
        };
        start(&mut session, &planned).unwrap()
    };
    assert!(run.interrupted);
    let index = root.path().join(".agentos/runs/index.json");
    std::fs::write(&index, b"{").unwrap();
    let restored = recover_index(root.path()).unwrap();
    assert!(restored >= 1);
    let loaded = load_run(root.path(), &run.id).unwrap();
    assert!(loaded.interrupted);
    let run_file = root
        .path()
        .join(".agentos/runs")
        .join(&run.id)
        .join("run.json");
    std::fs::write(&run_file, b"{").unwrap();
    let err = load_run(root.path(), &run.id).unwrap_err();
    assert!(matches!(err, WorkflowError::Store(_)), "{err}");
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
