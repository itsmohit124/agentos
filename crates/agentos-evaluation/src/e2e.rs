//! Golden workflows.
//!
//! A case copies a fixture, retrieves context, plans the shipped feature workflow, and applies a
//! scripted patch when the `tdd` and `implementation` steps run. Tests, static checks, and
//! architecture analysis then run for real. A negative case passes only when that patch is
//! rejected.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use agentos_context::{ContextConfig, NullSemantic, SqliteView, build, intent_kind};
use agentos_core::fs::{FileSystem, RealFs};
use agentos_feedback::{ArchitectureConfig, analyze_project};
use agentos_knowledge::{KnowledgeStore, domain_scan, scan};
use agentos_skills::{SkillLoader, builtin};
use agentos_workflow::{
    AgentRunner, EngineContext, MockReaction, PlanInput, RunMode, RunnerResult, ScriptedValidator,
    Session, StepRequest, WorkflowError, plan, start,
};
use serde::Deserialize;

use crate::{CaseResult, EvalError, EvalReport, EvalTrace, TraceExclusion};

const TASK: &str = "Add retry support to payment webhook processing";
const REQUIRED_STEPS: &[&str] = &[
    "research",
    "domain_design",
    "interface",
    "tdd",
    "implementation",
    "unit_test",
    "architecture_review",
    "done",
];

/// Runs every case in `cases_dir`.
pub fn run(cases_dir: &Path, fixtures_dir: &Path) -> Result<EvalReport, EvalError> {
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
        if case.task != TASK {
            return Err(EvalError::InvalidCase {
                path,
                message: format!("task must be {TASK:?}"),
            });
        }
        if case.oracle != "golden-workflow" {
            return Err(EvalError::InvalidCase {
                path,
                message: "oracle must be golden-workflow".to_owned(),
            });
        }
        cases.push(execute(&case, fixtures_dir));
    }
    Ok(EvalReport {
        area: "e2e".to_owned(),
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
    fixture: String,
    oracle: String,
    task: String,
    expect: String,
    patch: String,
    must_include: Vec<String>,
    must_exclude: Vec<String>,
    required_fragments: Vec<String>,
    terminology: Vec<String>,
    unrelated: Vec<String>,
}

struct Verdict {
    harness_ok: bool,
    golden_ok: bool,
    detail: String,
    trace: EvalTrace,
}

fn execute(case: &Case, fixtures_dir: &Path) -> CaseResult {
    let verdict = match evaluate(case, fixtures_dir) {
        Ok(verdict) => verdict,
        Err(err) => Verdict {
            harness_ok: false,
            golden_ok: false,
            detail: err,
            trace: EvalTrace {
                inputs: vec![
                    format!("fixture={}", case.fixture),
                    format!("task={}", case.task),
                ],
                selected: Vec::new(),
                excluded: Vec::new(),
                tools: Vec::new(),
                permissions: Vec::new(),
                validation: vec!["harness error".to_owned()],
            },
        },
    };
    let expect_pass = case.expect == "pass";
    let passed = verdict.harness_ok && verdict.golden_ok == expect_pass;
    let mut detail = verdict.detail;
    if !verdict.harness_ok {
        detail.push_str("; harness error");
    }
    detail.push_str(&format!(
        "; golden {}; expect {}",
        if verdict.golden_ok { "pass" } else { "fail" },
        case.expect
    ));
    CaseResult {
        id: case.id.clone(),
        kind: case.kind.clone(),
        description: case.description.clone(),
        passed,
        detail,
        trace: Some(verdict.trace),
    }
}

fn evaluate(case: &Case, fixtures_dir: &Path) -> Result<Verdict, String> {
    if case.kind != "golden" {
        return Err(format!("unknown kind {}", case.kind));
    }
    if case.expect != "pass" && case.expect != "fail" {
        return Err(format!("expect must be pass or fail, got {}", case.expect));
    }
    if case.patch != "positive" && case.patch != "negative" {
        return Err(format!(
            "patch must be positive or negative, got {}",
            case.patch
        ));
    }
    let source = fixtures_dir.join(&case.fixture);
    if !source.is_dir() {
        return Err(format!("fixture {} is missing", source.display()));
    }
    let copy = TempCopy::of(&source)?;
    let root = copy.path().to_path_buf();
    let before = snapshot(&root, &case.unrelated)?;
    scan(&root).map_err(|err| err.to_string())?;
    domain_scan(&root).map_err(|err| err.to_string())?;
    let store = KnowledgeStore::open(&root).map_err(|err| err.to_string())?;
    let view = SqliteView::new(&store);
    let config = ContextConfig::default();
    let pack = build(&view, &case.task, &config, &NullSemantic).map_err(|err| err.to_string())?;
    let (intent, intent_rule) = intent_kind(&case.task);
    let findings_before = finding_ids(&root)?;

    let discovery =
        SkillLoader::for_project(&root, Arc::new(RealFs), None, builtin::location()).discover();
    let planned = plan(&PlanInput {
        task: &case.task,
        workflow: Some("feature"),
        definition: None,
        registry: &discovery.registry,
        concurrency: None,
        max_fix_iterations: None,
    })
    .map_err(|err| err.to_string())?;
    let missing_steps: Vec<&str> = REQUIRED_STEPS
        .iter()
        .copied()
        .filter(|id| !planned.steps.iter().any(|step| step.id == *id))
        .collect();
    if !missing_steps.is_empty() {
        return Err(format!("plan is missing steps {missing_steps:?}"));
    }

    let language = language_of(&case.fixture)?;
    let python = source.join(".venv/bin/python");
    let python = python.is_file().then_some(python);
    let edits = edits_for(language, &case.patch);
    let mut runner = PatchRunner {
        root: root.clone(),
        edits,
        applied: Vec::new(),
        calls: Vec::new(),
    };
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
            root: &root,
            runner: &mut runner,
            validator: &mut validator,
            context: &context,
        };
        start(&mut session, &planned).map_err(|err| err.to_string())?
    };

    let selected_ids: Vec<&str> = pack
        .selected_nodes
        .iter()
        .map(|node| node.id.as_str())
        .collect();
    let missing: Vec<&str> = case
        .must_include
        .iter()
        .filter(|id| !selected_ids.contains(&id.as_str()))
        .map(String::as_str)
        .collect();
    let forbidden: Vec<String> = pack
        .selected_nodes
        .iter()
        .filter(|node| case.must_exclude.contains(&node.id) || banned_package(&node.id))
        .map(|node| node.id.clone())
        .collect();
    let fragment_sources: Vec<&str> = pack
        .selected_fragments
        .iter()
        .map(|fragment| fragment.source_ref.as_str())
        .collect();
    let missing_fragments: Vec<&str> = case
        .required_fragments
        .iter()
        .filter(|path| !fragment_sources.contains(&path.as_str()))
        .map(String::as_str)
        .collect();
    let patch_text = patched_text(&root, language);
    let terminology_ok = case.terminology.iter().all(|term| {
        patch_text.contains(term)
            && (pack.terminology.iter().any(|entry| entry.name == *term)
                || pack
                    .selected_nodes
                    .iter()
                    .any(|node| node.name == *term || node.id.contains(term)))
    });
    let context_ok = missing.is_empty() && forbidden.is_empty() && missing_fragments.is_empty();
    let aggregate_ok = aggregate_respected(&root, language);
    let unrelated_ok = snapshot(&root, &case.unrelated)? == before;
    let tests = run_tests(&root, language, python.as_deref());
    let static_analysis = run_static(&root, language, python.as_deref());
    let findings_after = finding_ids(&root)?;
    let introduced: Vec<String> = findings_after
        .iter()
        .filter(|id| !findings_before.contains(id))
        .cloned()
        .collect();
    let architecture_ok = introduced.is_empty();
    let tests_ok = tests.ok;
    let static_ok = static_analysis.ok;
    let golden_ok = context_ok
        && terminology_ok
        && aggregate_ok
        && unrelated_ok
        && tests_ok
        && static_ok
        && architecture_ok
        && intent == "feature";

    let mut detail = format!(
        "intent {intent} ({intent_rule}); context missing {missing:?} forbidden {forbidden:?} fragments {missing_fragments:?}; terminology {terminology_ok}; aggregate {aggregate_ok}; unrelated {unrelated_ok}; tests {}; static {}; architecture introduced {introduced:?}; workflow {}",
        tests.summary,
        static_analysis.summary,
        run.state.as_str()
    );
    if !runner
        .calls
        .iter()
        .any(|call| call == "implementation:execute")
    {
        detail.push_str("; implementation step did not run");
    }

    let mut selected = selected_ids
        .iter()
        .map(|id| (*id).to_owned())
        .collect::<Vec<_>>();
    selected.sort();
    let mut excluded: Vec<TraceExclusion> = pack
        .exclusions
        .iter()
        .map(|item| TraceExclusion {
            id: item.id.clone(),
            reason: item.reason.clone(),
        })
        .collect();
    for skipped in &planned.skipped {
        excluded.push(TraceExclusion {
            id: skipped.id.clone(),
            reason: skipped.reason.clone(),
        });
    }
    excluded.sort_by(|left, right| left.id.cmp(&right.id).then(left.reason.cmp(&right.reason)));
    let mut tools = vec![
        "context-engine".to_owned(),
        "workflow-planner".to_owned(),
        "scripted-patch".to_owned(),
        tests.tool.clone(),
        static_analysis.tool.clone(),
        "architecture-analyze".to_owned(),
    ];
    tools.sort();
    let mut permissions = planned.permissions.clone();
    permissions.sort();
    let validation = vec![
        format!("workflow-state {}", run.state.as_str()),
        format!("scripted-validator {}", validator.calls.join(",")),
        format!("tests {}", tests.summary),
        format!("static {}", static_analysis.summary),
        format!("architecture-new {}", introduced.len()),
        "scripted validator does not spawn a process; tests and architecture analysis are separate evidence".to_owned(),
    ];
    let trace = EvalTrace {
        inputs: vec![
            format!("fixture={}", case.fixture),
            format!("task={}", case.task),
            format!("patch={}", case.patch),
            format!("workflow={}", planned.workflow),
            format!("intent={intent}"),
        ],
        selected,
        excluded,
        tools,
        permissions,
        validation,
    };
    Ok(Verdict {
        harness_ok: runner
            .calls
            .iter()
            .any(|call| call == "implementation:execute"),
        golden_ok,
        detail,
        trace,
    })
}

fn language_of(fixture: &str) -> Result<&'static str, String> {
    match fixture {
        "python-fastapi" => Ok("python"),
        "node-express" => Ok("node"),
        other => Err(format!("no golden patch for fixture {other}")),
    }
}

fn aggregate_respected(root: &Path, language: &str) -> bool {
    match language {
        "python" => {
            let service = read_lossy(&root.join("app/payments/service.py"));
            let models = read_lossy(&root.join("app/payments/models.py"));
            service.contains("def retry_webhook")
                && service.contains("PaymentAggregate")
                && !models.contains("import sqlite3")
        }
        "node" => {
            let service = read_lossy(&root.join("app/payments/service.js"));
            let models = read_lossy(&root.join("app/payments/models.js"));
            service.contains("retryWebhook") && !models.contains("PaymentRepository")
        }
        _ => false,
    }
}

fn edits_for(language: &str, patch: &str) -> Vec<Edit> {
    match (language, patch) {
        ("python", "positive") => vec![python_test_edit(), python_service_edit()],
        ("python", "negative") => vec![
            python_test_edit(),
            python_service_edit(),
            Edit::Prepend {
                path: "app/payments/models.py".to_owned(),
                text: "import sqlite3\n".to_owned(),
            },
            Edit::Append {
                path: "app/users/service.py".to_owned(),
                text: "\nUNRELATED = \"edited\"\n".to_owned(),
            },
        ],
        ("node", "positive") => vec![node_test_edit(), node_service_edit()],
        ("node", "negative") => vec![
            node_test_edit(),
            node_service_edit(),
            Edit::Prepend {
                path: "app/payments/models.js".to_owned(),
                text: "import { PaymentRepository } from \"./repository.js\";\n".to_owned(),
            },
            Edit::Append {
                path: "app/users/service.js".to_owned(),
                text: "\nexport const unrelatedEdit = true;\n".to_owned(),
            },
        ],
        _ => Vec::new(),
    }
}

fn python_test_edit() -> Edit {
    Edit::Replace {
        path: "tests/test_payments.py".to_owned(),
        from: "from app.payments.models import Payment\n".to_owned(),
        to: "from app.payments.models import Payment, PaymentAggregate\n".to_owned(),
        append: r#"

def test_webhook_retry_uses_payment_aggregate() -> None:
    """Retry uses the payment aggregate and the existing Payment term."""
    payment = Payment("pay-2", 100)
    aggregate = PaymentAggregate(payment)
    service = PaymentService()
    retried = service.retry_webhook(aggregate, 3)
    assert retried.payment_id == "pay-2"
    assert retried.amount == 100
"#
        .to_owned(),
    }
}

fn python_service_edit() -> Edit {
    Edit::Append {
        path: "app/payments/service.py".to_owned(),
        text: r#"
    def retry_webhook(self, aggregate: PaymentAggregate, attempts: int) -> Payment:
        """Retry payment webhook processing through the aggregate."""
        if attempts <= 0:
            raise ValueError("attempts > 0")
        payment = aggregate.root
        return payment
"#
        .to_owned(),
    }
}

fn node_test_edit() -> Edit {
    Edit::Append {
        path: "tests/test_payments.js".to_owned(),
        text: r#"
export function test_webhook_retry_uses_payment() {
  const payment = new Payment("pay-2", 100);
  const service = new PaymentService();
  const retried = service.retryWebhook(payment, 3);
  if (retried.paymentId !== "pay-2" || retried.amount !== 100) {
    throw new Error("retry did not return the payment");
  }
}
"#
        .to_owned(),
    }
}

fn node_service_edit() -> Edit {
    Edit::InsertBeforeLastBrace {
        path: "app/payments/service.js".to_owned(),
        text: r#"
  retryWebhook(payment, attempts) {
    if (attempts <= 0) {
      throw new Error("attempts > 0");
    }
    return payment;
  }
"#
        .to_owned(),
    }
}

fn patched_text(root: &Path, language: &str) -> String {
    let paths = match language {
        "python" => ["app/payments/service.py", "tests/test_payments.py"],
        _ => ["app/payments/service.js", "tests/test_payments.js"],
    };
    paths
        .iter()
        .map(|path| read_lossy(&root.join(path)))
        .collect::<Vec<_>>()
        .join("\n")
}

struct Check {
    ok: bool,
    summary: String,
    tool: String,
}

fn run_tests(root: &Path, language: &str, python: Option<&Path>) -> Check {
    match language {
        "python" => {
            let script = r#"
import sys
sys.path.insert(0, ".")
import tests.test_payments as tests
names = sorted(name for name in dir(tests) if name.startswith("test_"))
if "test_webhook_retry_uses_payment_aggregate" not in names:
    raise SystemExit("missing retry test")
for name in names:
    getattr(tests, name)()
print("passed " + str(len(names)))
"#;
            run_python(root, python, &["-c", script], "python-tests")
        }
        "node" => {
            let script = r#"
import * as tests from "./tests/test_payments.js";
const names = Object.keys(tests).filter((name) => name.startsWith("test_")).sort();
if (!names.includes("test_webhook_retry_uses_payment")) {
  throw new Error("missing retry test");
}
for (const name of names) {
  tests[name]();
}
console.log("passed " + names.length);
"#;
            run_program(
                root,
                &["node"],
                &["--input-type=module", "-e", script],
                "node-tests",
            )
        }
        other => Check {
            ok: false,
            summary: format!("no test runner for {other}"),
            tool: "none".to_owned(),
        },
    }
}

fn run_static(root: &Path, language: &str, python: Option<&Path>) -> Check {
    match language {
        "python" => run_python(
            root,
            python,
            &[
                "-m",
                "py_compile",
                "app/payments/service.py",
                "app/payments/models.py",
                "tests/test_payments.py",
            ],
            "py_compile",
        ),
        "node" => {
            let service = run_program(
                root,
                &["node"],
                &["--check", "app/payments/service.js"],
                "node-check",
            );
            if !service.ok {
                return service;
            }
            run_program(
                root,
                &["node"],
                &["--check", "app/payments/models.js"],
                "node-check",
            )
        }
        other => Check {
            ok: false,
            summary: format!("no static check for {other}"),
            tool: "none".to_owned(),
        },
    }
}

fn run_python(root: &Path, python: Option<&Path>, args: &[&str], tool: &str) -> Check {
    if let Some(python) = python {
        return run_program(root, &[python.to_str().unwrap_or("python3")], args, tool);
    }
    // The fixture modules annotate names they do not import. Python 3.14 defers those
    // annotations. An older `python3` needs the future import before it can load them.
    for relative in [
        "app/payments/models.py",
        "app/payments/repository.py",
        "app/payments/service.py",
        "tests/test_payments.py",
    ] {
        let path = root.join(relative);
        if let Ok(body) = fs::read_to_string(&path)
            && !body.contains("from __future__ import annotations")
        {
            let _ = fs::write(&path, format!("from __future__ import annotations\n{body}"));
        }
    }
    run_program(root, &["python3", "python"], args, tool)
}

fn run_program(root: &Path, programs: &[&str], args: &[&str], tool: &str) -> Check {
    let Some(program) = programs.iter().find(|program| which(program)) else {
        return Check {
            ok: false,
            summary: format!("{tool} missing {}", programs.join(" or ")),
            tool: tool.to_owned(),
        };
    };
    let output = Command::new(program).args(args).current_dir(root).output();
    match output {
        Ok(output) if output.status.success() => Check {
            ok: true,
            summary: format!("{tool} exit 0"),
            tool: tool.to_owned(),
        },
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            let message = one_line(&format!("{stdout} {stderr}"));
            Check {
                ok: false,
                summary: format!("{tool} exit {:?} {message}", output.status.code()),
                tool: tool.to_owned(),
            }
        }
        Err(err) => Check {
            ok: false,
            summary: format!("{tool} {err}"),
            tool: tool.to_owned(),
        },
    }
}

fn which(program: &str) -> bool {
    Command::new(program)
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
}

fn finding_ids(root: &Path) -> Result<Vec<String>, String> {
    let report =
        analyze_project(root, &ArchitectureConfig::default()).map_err(|err| err.to_string())?;
    let mut ids: Vec<String> = report
        .findings
        .into_iter()
        .map(|finding| finding.id)
        .collect();
    ids.sort();
    Ok(ids)
}

fn banned_package(id: &str) -> bool {
    id.contains(".notifications.")
        || id.contains(".users.")
        || id.contains(".fleet.")
        || id.contains(".analytics.")
}

fn snapshot(root: &Path, paths: &[String]) -> Result<Vec<(String, Vec<u8>)>, String> {
    let mut out = Vec::new();
    for relative in paths {
        let bytes = fs::read(root.join(relative))
            .map_err(|err| format!("cannot read {relative}: {err}"))?;
        out.push((relative.clone(), bytes));
    }
    Ok(out)
}

fn read_lossy(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_default()
}

fn one_line(text: &str) -> String {
    let mut line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.len() > 180 {
        line.truncate(180);
    }
    line
}

enum Edit {
    Append {
        path: String,
        text: String,
    },
    Prepend {
        path: String,
        text: String,
    },
    InsertBeforeLastBrace {
        path: String,
        text: String,
    },
    Replace {
        path: String,
        from: String,
        to: String,
        append: String,
    },
}

struct PatchRunner {
    root: PathBuf,
    edits: Vec<Edit>,
    applied: Vec<String>,
    calls: Vec<String>,
}

impl PatchRunner {
    fn apply(&mut self, step_id: &str) -> Result<(), String> {
        if self.applied.iter().any(|id| id == step_id) {
            return Ok(());
        }
        let matched: Vec<usize> = match step_id {
            "tdd" => self
                .edits
                .iter()
                .enumerate()
                .filter(|(_, edit)| edit_path(edit).starts_with("tests/"))
                .map(|(index, _)| index)
                .collect(),
            "implementation" => self
                .edits
                .iter()
                .enumerate()
                .filter(|(_, edit)| !edit_path(edit).starts_with("tests/"))
                .map(|(index, _)| index)
                .collect(),
            _ => Vec::new(),
        };
        for index in matched {
            apply_edit(&self.root, &self.edits[index])?;
        }
        self.applied.push(step_id.to_owned());
        Ok(())
    }
}

fn edit_path(edit: &Edit) -> &str {
    match edit {
        Edit::Append { path, .. }
        | Edit::Prepend { path, .. }
        | Edit::InsertBeforeLastBrace { path, .. }
        | Edit::Replace { path, .. } => path,
    }
}

fn apply_edit(root: &Path, edit: &Edit) -> Result<(), String> {
    match edit {
        Edit::Append { path, text } => {
            let full = root.join(path);
            let mut body = fs::read_to_string(&full).map_err(|err| err.to_string())?;
            body.push_str(text);
            fs::write(&full, body).map_err(|err| err.to_string())
        }
        Edit::Prepend { path, text } => {
            let full = root.join(path);
            let body = fs::read_to_string(&full).map_err(|err| err.to_string())?;
            fs::write(&full, format!("{text}{body}")).map_err(|err| err.to_string())
        }
        Edit::InsertBeforeLastBrace { path, text } => {
            let full = root.join(path);
            let body = fs::read_to_string(&full).map_err(|err| err.to_string())?;
            let Some(index) = body.rfind('}') else {
                return Err(format!("{path} has no closing brace"));
            };
            let mut next = String::new();
            next.push_str(&body[..index]);
            next.push_str(text);
            next.push_str(&body[index..]);
            fs::write(&full, next).map_err(|err| err.to_string())
        }
        Edit::Replace {
            path,
            from,
            to,
            append,
        } => {
            let full = root.join(path);
            let body = fs::read_to_string(&full).map_err(|err| err.to_string())?;
            if !body.contains(from) {
                return Err(format!("{path} is missing {from:?}"));
            }
            let mut next = body.replacen(from, to, 1);
            next.push_str(append);
            fs::write(full, next).map_err(|err| err.to_string())
        }
    }
}

impl AgentRunner for PatchRunner {
    fn run(&mut self, request: &StepRequest) -> Result<RunnerResult, WorkflowError> {
        self.calls
            .push(format!("{}:{}", request.step_id, request.mode.as_str()));
        if request.mode == RunMode::Execute {
            self.apply(&request.step_id)
                .map_err(WorkflowError::Context)?;
        }
        let reaction = MockReaction::ok();
        let mut outputs = reaction.outputs;
        for name in &request.outputs {
            outputs.insert(name.clone(), format!("{name}:ok"));
        }
        Ok(RunnerResult {
            failed: false,
            message: "scripted patch".to_owned(),
            outputs,
            edits: Vec::new(),
        })
    }
}

struct TempCopy {
    path: PathBuf,
}

impl TempCopy {
    fn of(source: &Path) -> Result<Self, String> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("agentos-e2e-{n}-{}", std::process::id()));
        if path.exists() {
            fs::remove_dir_all(&path).map_err(|err| err.to_string())?;
        }
        copy_tree(source, &path)
            .map_err(|err| format!("cannot copy {}: {err}", source.display()))?;
        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempCopy {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    fs::create_dir_all(to)?;
    let mut entries: Vec<_> = fs::read_dir(from)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let name = entry.file_name();
        if name == ".agentos"
            || name == ".venv"
            || name == "node_modules"
            || name == "__pycache__"
            || name == ".pytest_cache"
            || name == ".git"
        {
            continue;
        }
        let target = to.join(&name);
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else if file_type.is_file() {
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}
