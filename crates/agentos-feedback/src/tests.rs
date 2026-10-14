//! Contract tests for validation results, parsers, permissions, and architecture detectors.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use agentos_knowledge::{
    KnowledgeEdge, KnowledgeFragment, KnowledgeNode, NodeKind, Provenance, ProvenanceKind,
    RelationType,
};
use agentos_permissions::{Capability, PermissionSet, PermissionSubject};
use agentos_tools::{ExecOutcome, ExecRequest, ProcessRunner, ToolRunner};
use tempfile::tempdir;

use crate::architecture::{GraphSnapshot, analyze};
use crate::config::{ArchitectureConfig, ProjectConfig, ValidationConfig, load_project_config};
use crate::error::FeedbackError;
use crate::kind::ValidationKind;
use crate::provider::{RunRequest, ValidationProvider};
use crate::providers::{
    CargoTestProvider, GoTestProvider, GradleProvider, JestProvider, MavenProvider, PytestProvider,
    RSpecProvider,
};
use crate::result::{CapturedOutput, Outcome, ValidationResult, refused};
use crate::store::{last_result, store_result};
use crate::{check_regression_case, detect, record_regression, run_and_store};

struct Subject {
    name: &'static str,
    permissions: PermissionSet,
}

impl PermissionSubject for Subject {
    fn subject(&self) -> String {
        self.name.to_owned()
    }
    fn permissions(&self) -> &PermissionSet {
        &self.permissions
    }
}

fn allow() -> Subject {
    Subject {
        name: "operator validate",
        permissions: [Capability::Shell].into_iter().collect(),
    }
}

fn deny() -> Subject {
    Subject {
        name: "skill 'silent'",
        permissions: PermissionSet::none(),
    }
}

fn captured(stdout: &str, stderr: &str, exit: Option<i32>) -> CapturedOutput {
    CapturedOutput {
        target: "/proj".into(),
        command: vec!["tool".into()],
        exit_code: exit,
        stdout: stdout.into(),
        stderr: stderr.into(),
        duration_ms: 10,
        timed_out: false,
        crashed: false,
        spawn_error: None,
        truncated: false,
        task_id: None,
        context_pack_id: None,
    }
}

const RESULT_KEYS: &[&str] = &[
    "id",
    "provider",
    "kind",
    "target",
    "command",
    "capability",
    "exit_status",
    "passed",
    "outcome",
    "duration_ms",
    "diagnostics",
    "raw_output_ref",
    "artifacts",
    "provenance",
    "task_id",
    "context_pack_id",
    "feedback",
    "output_truncated",
];

#[test]
fn schema_keeps_required_fields_when_values_are_absent() {
    let result = refused(
        "pytest",
        ValidationKind::UnitTest,
        "/proj",
        vec!["python3".into(), "-m".into(), "pytest".into()],
        "skill 'silent' does not declare shell (default deny)",
        None,
        None,
    );
    let value = serde_json::to_value(&result).unwrap();
    for key in RESULT_KEYS {
        assert!(value.get(key).is_some(), "missing {key}");
    }
    assert!(value["exit_status"].is_null());
    assert!(value["raw_output_ref"].is_null());
    assert!(value["task_id"].is_null());
    assert!(value["context_pack_id"].is_null());
    assert_eq!(value["passed"], false);
    assert_eq!(value["outcome"], "refused");
    assert_eq!(value["capability"], "shell");
    assert_eq!(value["duration_ms"], 0);
    let item = &value["feedback"][0];
    for key in [
        "id",
        "validation_id",
        "provider",
        "kind",
        "summary",
        "file",
        "line",
        "task_id",
        "context_pack_id",
        "provenance",
    ] {
        assert!(item.get(key).is_some(), "feedback missing {key}");
    }
    assert!(item["file"].is_null());
    assert!(item["line"].is_null());
    let provenance = &value["provenance"];
    for key in [
        "kind",
        "source",
        "location",
        "extractor",
        "extractor_version",
        "method",
    ] {
        assert!(provenance.get(key).is_some(), "provenance missing {key}");
    }
}

#[test]
fn parsers_cover_pass_fail_timeout_and_crash() {
    let cases: &[(&str, ValidationResult, &str, u32)] = &[];
    let _ = cases;
    let samples = [
        (
            "pytest",
            PytestProvider::parse(
                ValidationKind::UnitTest,
                &captured("2 passed in 0.01s\n", "", Some(0)),
            ),
            Outcome::Passed,
        ),
        (
            "cargo",
            CargoTestProvider::parse(
                ValidationKind::UnitTest,
                &captured("test result: ok. 1 passed; 0 failed\n", "", Some(0)),
            ),
            Outcome::Passed,
        ),
        (
            "jest",
            JestProvider::parse(
                ValidationKind::UnitTest,
                &captured("Tests: 1 passed\n", "", Some(0)),
            ),
            Outcome::Passed,
        ),
        (
            "go",
            GoTestProvider::parse(
                ValidationKind::UnitTest,
                &captured("ok  example.com/goapp\n", "", Some(0)),
            ),
            Outcome::Passed,
        ),
        (
            "maven",
            MavenProvider::parse(
                ValidationKind::UnitTest,
                &captured("BUILD SUCCESS\n", "", Some(0)),
            ),
            Outcome::Passed,
        ),
        (
            "gradle",
            GradleProvider::parse(
                ValidationKind::UnitTest,
                &captured("BUILD SUCCESSFUL\n", "", Some(0)),
            ),
            Outcome::Passed,
        ),
        (
            "rspec",
            RSpecProvider::parse(
                ValidationKind::UnitTest,
                &captured("1 example, 0 failures\n", "", Some(0)),
            ),
            Outcome::Passed,
        ),
    ];
    for (name, result, outcome) in samples {
        assert_eq!(result.outcome, outcome, "{name} pass");
        assert!(result.passed, "{name}");
        assert!(result.feedback.is_empty(), "{name}");
    }

    let failed = PytestProvider::parse(
        ValidationKind::UnitTest,
        &captured(
            "tests/test_seed.py:2: AssertionError\nFAILED tests/test_seed.py::test_amount\n",
            "",
            Some(1),
        ),
    );
    assert_eq!(failed.outcome, Outcome::Failed);
    assert!(!failed.passed);
    assert_eq!(
        failed.diagnostics[0].file.as_deref(),
        Some("tests/test_seed.py")
    );
    assert_eq!(failed.diagnostics[0].line, Some(2));
    assert!(!failed.feedback.is_empty());

    let cargo = CargoTestProvider::parse(
        ValidationKind::UnitTest,
        &captured(
            "thread 'tests::fails' panicked at src/lib.rs:7:9:\nassertion failed: amount > 0\n",
            "",
            Some(101),
        ),
    );
    assert_eq!(cargo.diagnostics[0].file.as_deref(), Some("src/lib.rs"));
    assert_eq!(cargo.diagnostics[0].line, Some(7));
    assert!(cargo.diagnostics[0].summary.contains("amount > 0"));

    let jest = JestProvider::parse(
        ValidationKind::UnitTest,
        &captured(
            "  ● amount must be positive\n\n    expect(received).toBe(expected)\n\n      at Object.<anonymous> (src/sum.test.js:4:19)\n",
            "",
            Some(1),
        ),
    );
    assert_eq!(jest.diagnostics[0].file.as_deref(), Some("src/sum.test.js"));
    assert_eq!(jest.diagnostics[0].line, Some(4));
    assert!(
        jest.diagnostics[0]
            .summary
            .contains("amount must be positive")
    );

    let go = GoTestProvider::parse(
        ValidationKind::UnitTest,
        &captured(
            "--- FAIL: TestCharge (0.00s)\n    add_test.go:11: amount must be positive\nFAIL\n",
            "",
            Some(1),
        ),
    );
    assert_eq!(go.diagnostics[0].file.as_deref(), Some("add_test.go"));
    assert_eq!(go.diagnostics[0].line, Some(11));

    let maven = MavenProvider::parse(
        ValidationKind::UnitTest,
        &captured(
            "\tat com.example.PaymentTest.testCharge(PaymentTest.java:18)\n",
            "",
            Some(1),
        ),
    );
    assert_eq!(
        maven.diagnostics[0].file.as_deref(),
        Some("PaymentTest.java")
    );
    assert_eq!(maven.diagnostics[0].line, Some(18));

    let gradle = GradleProvider::parse(
        ValidationKind::UnitTest,
        &captured(
            "        at com.example.PaymentTest.charge(PaymentTest.java:18)\n",
            "",
            Some(1),
        ),
    );
    assert_eq!(
        gradle.diagnostics[0].file.as_deref(),
        Some("PaymentTest.java")
    );
    assert_eq!(gradle.diagnostics[0].line, Some(18));

    let rspec = RSpecProvider::parse(
        ValidationKind::UnitTest,
        &captured("     # ./spec/payment_spec.rb:12:in `block'\n", "", Some(1)),
    );
    assert_eq!(
        rspec.diagnostics[0].file.as_deref(),
        Some("./spec/payment_spec.rb")
    );
    assert_eq!(rspec.diagnostics[0].line, Some(12));

    let mut timed = captured("2 passed in 0.01s\n", "", None);
    timed.timed_out = true;
    let timed = PytestProvider::parse(ValidationKind::UnitTest, &timed);
    assert_eq!(timed.outcome, Outcome::TimedOut);
    assert!(!timed.passed);

    let mut crashed = captured("", "", None);
    crashed.crashed = true;
    let crashed = CargoTestProvider::parse(ValidationKind::UnitTest, &crashed);
    assert_eq!(crashed.outcome, Outcome::Crashed);
    assert!(!crashed.passed);

    let mut missing = captured("", "", None);
    missing.spawn_error = Some("No such file or directory".into());
    let missing = JestProvider::parse(ValidationKind::UnitTest, &missing);
    assert_eq!(missing.outcome, Outcome::Unavailable);
    assert!(!missing.passed);
}

#[test]
fn command_construction_matches_the_toolchain() {
    assert_eq!(
        CargoTestProvider::command(ValidationKind::UnitTest, None).unwrap(),
        ["cargo", "test", "--offline", "--color", "never"]
    );
    assert_eq!(
        PytestProvider::command(Path::new("."), ValidationKind::UnitTest, None).unwrap(),
        ["python3", "-m", "pytest", "-q", "--tb=short", "--color=no"]
    );
    let venv_root = tempdir().unwrap();
    let venv_python = venv_root.path().join(".venv/bin/python");
    fs::create_dir_all(venv_python.parent().unwrap()).unwrap();
    fs::write(&venv_python, "").unwrap();
    assert_eq!(
        PytestProvider::command(venv_root.path(), ValidationKind::UnitTest, None).unwrap()[0],
        venv_python.display().to_string()
    );
    assert_eq!(
        GoTestProvider::command(ValidationKind::UnitTest, None).unwrap(),
        ["go", "test", "./..."]
    );
    assert_eq!(
        MavenProvider::command(ValidationKind::UnitTest, None).unwrap(),
        ["mvn", "-o", "-q", "test"]
    );
    let root = tempdir().unwrap();
    fs::write(root.path().join("build.gradle"), "").unwrap();
    assert_eq!(
        GradleProvider::command(root.path(), ValidationKind::UnitTest, None).unwrap(),
        ["gradle", "--offline", "test"]
    );
    fs::write(root.path().join("Gemfile"), "gem \"rspec\"\n").unwrap();
    assert_eq!(
        RSpecProvider::command(root.path(), None).unwrap(),
        ["bundle", "exec", "rspec"]
    );
    fs::write(
        root.path().join("package.json"),
        r#"{"scripts":{"test":"jest --watchAll=false"},"devDependencies":{"jest":"29.7.0"}}"#,
    )
    .unwrap();
    assert_eq!(
        JestProvider::command(root.path(), None).unwrap(),
        [
            "npm",
            "test",
            "--silent",
            "--",
            "--watchAll=false",
            "--colors=false"
        ]
    );
    assert!(CargoTestProvider::command(ValidationKind::Architecture, None).is_err());
}

#[test]
fn permission_refusal_does_not_spawn() {
    let dir = tempdir().unwrap();
    let marker = dir.path().join("marker");
    let request = RunRequest {
        root: dir.path(),
        kind: ValidationKind::UnitTest,
        tools: &ProcessRunner,
        subject: &deny(),
        task_id: Some("task-1".into()),
        context_pack_id: Some("pack-1".into()),
        command_override: Some(format!("/usr/bin/touch {}", marker.display())),
        timeout: Duration::from_secs(2),
        max_output_bytes: 1024,
    };
    let result = CargoTestProvider.run(&request).unwrap();
    assert_eq!(result.outcome, Outcome::Refused);
    assert!(!result.passed);
    assert!(result.exit_status.is_none());
    assert_eq!(result.task_id.as_deref(), Some("task-1"));
    assert_eq!(result.context_pack_id.as_deref(), Some("pack-1"));
    assert!(!marker.exists());
    assert!(result.feedback[0].summary.contains("shell"));
}

#[test]
fn timeout_nonzero_crash_and_missing_program_are_not_passes() {
    let dir = tempdir().unwrap();
    let subject = allow();
    let run = |command: &str, timeout_ms: u64, cap: usize| {
        let request = RunRequest {
            root: dir.path(),
            kind: ValidationKind::UnitTest,
            tools: &ProcessRunner,
            subject: &subject,
            task_id: None,
            context_pack_id: None,
            command_override: Some(command.to_owned()),
            timeout: Duration::from_millis(timeout_ms),
            max_output_bytes: cap,
        };
        CargoTestProvider.run(&request).unwrap()
    };
    let timed = run("/bin/sleep 5", 200, 1024);
    assert_eq!(timed.outcome, Outcome::TimedOut);
    assert!(!timed.passed);
    let failed = run("/usr/bin/false", 2_000, 1024);
    assert_eq!(failed.outcome, Outcome::Failed);
    assert_eq!(failed.exit_status, Some(1));
    assert!(!failed.passed);
    let missing = run("agentos-missing-binary-xyz", 1_000, 1024);
    assert_eq!(missing.outcome, Outcome::Unavailable);
    assert!(!missing.passed);
    let script = dir.path().join("crash.sh");
    fs::write(&script, "kill -ABRT $$\n").unwrap();
    let crashed = run(&format!("/bin/sh {}", script.display()), 2_000, 1024);
    assert_eq!(crashed.outcome, Outcome::Crashed, "{crashed:?}");
    let big = dir.path().join("big.txt");
    fs::write(&big, "x".repeat(10_000)).unwrap();
    let capped = run(&format!("/bin/cat {}", big.display()), 2_000, 64);
    assert!(capped.output_truncated);
    assert!(capped.passed);
}

#[test]
fn last_result_is_the_latest_stored_record() {
    let dir = tempdir().unwrap();
    let first = refused(
        "cargo",
        ValidationKind::UnitTest,
        dir.path().display().to_string().as_str(),
        vec!["cargo".into(), "test".into()],
        "first",
        None,
        None,
    );
    let stdout = "tests/test_seed.py:2: AssertionError\n";
    let second = PytestProvider::parse(ValidationKind::UnitTest, &captured(stdout, "", Some(1)));
    store_result(dir.path(), &first, "one", "").unwrap();
    let stored = store_result(dir.path(), &second, stdout, "").unwrap();
    let last = last_result(dir.path()).unwrap();
    assert_eq!(last.id, stored.id);
    assert_eq!(last.provider, "pytest");
    assert!(last.raw_output_ref.is_some());
    let case_dir = dir.path().join("evals/feedback");
    let path = record_regression(dir.path(), &stored.id, &case_dir).unwrap();
    let case: crate::RegressionCase =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    check_regression_case(&case).unwrap();
    let mut broken = case.clone();
    broken.expected_outcome = "passed".into();
    assert!(check_regression_case(&broken).is_err());
}

#[test]
fn pytest_exit_zero_with_a_location_stays_a_pass() {
    let passed = PytestProvider::parse(
        ValidationKind::UnitTest,
        &captured(
            "tests/test_ok.py:3: see the example\n1 passed in 0.01s\n",
            "",
            Some(0),
        ),
    );
    assert_eq!(passed.outcome, Outcome::Passed);
    assert!(passed.passed);
    assert!(passed.diagnostics.is_empty(), "{:?}", passed.diagnostics);
    assert!(passed.feedback.is_empty());

    let failed = PytestProvider::parse(
        ValidationKind::UnitTest,
        &captured("tests/test_seed.py:2: AssertionError\n", "", Some(1)),
    );
    assert_eq!(failed.outcome, Outcome::Failed);
    assert!(!failed.passed);
    assert_eq!(
        failed.diagnostics[0].file.as_deref(),
        Some("tests/test_seed.py")
    );
    assert_eq!(failed.diagnostics[0].line, Some(2));
    assert!(failed.diagnostics[0].summary.contains("AssertionError"));

    let mut timed = captured("tests/test_seed.py:2: AssertionError\n", "", None);
    timed.timed_out = true;
    let timed = PytestProvider::parse(ValidationKind::UnitTest, &timed);
    assert_eq!(timed.outcome, Outcome::TimedOut);
    assert_eq!(timed.diagnostics[0].line, Some(2));

    let mut crashed = captured("tests/test_seed.py:2: AssertionError\n", "", None);
    crashed.crashed = true;
    let crashed = PytestProvider::parse(ValidationKind::UnitTest, &crashed);
    assert_eq!(crashed.outcome, Outcome::Crashed);
    assert_eq!(
        crashed.diagnostics[0].file.as_deref(),
        Some("tests/test_seed.py")
    );

    let mut missing = captured("", "", None);
    missing.spawn_error = Some("No such file".into());
    let missing = PytestProvider::parse(ValidationKind::UnitTest, &missing);
    assert_eq!(missing.outcome, Outcome::Unavailable);
    assert!(!missing.passed);

    let nonzero = PytestProvider::parse(ValidationKind::UnitTest, &captured("", "", Some(1)));
    assert_eq!(nonzero.outcome, Outcome::Failed);
    assert_eq!(nonzero.exit_status, Some(1));
}

#[test]
fn refused_results_are_not_regression_cases() {
    let dir = tempdir().unwrap();
    let result = refused(
        "pytest",
        ValidationKind::UnitTest,
        dir.path().display().to_string().as_str(),
        vec!["python3".into(), "-m".into(), "pytest".into()],
        "shell denied",
        None,
        None,
    );
    let stored = store_result(dir.path(), &result, "", "").unwrap();
    let err = record_regression(dir.path(), &stored.id, &dir.path().join("evals")).unwrap_err();
    assert!(matches!(err, FeedbackError::NotReplayable(_)), "{err}");
    assert!(
        !dir.path()
            .join("evals")
            .join(format!("{}.json", stored.id))
            .exists()
    );
}

struct WaitErrorRunner;

impl ToolRunner for WaitErrorRunner {
    fn execute(
        &self,
        _subject: &dyn PermissionSubject,
        request: &ExecRequest,
    ) -> Result<ExecOutcome, agentos_tools::ToolError> {
        Ok(ExecOutcome {
            program: request.program.clone(),
            args: request.args.clone(),
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            stdout_truncated: false,
            stderr_truncated: false,
            duration_ms: 1,
            timed_out: false,
            spawn_error: None,
            wait_error: Some("try_wait: broken pipe".into()),
        })
    }
}

#[test]
fn a_wait_error_is_unavailable_not_a_crash() {
    let dir = tempdir().unwrap();
    let result = CargoTestProvider
        .run(&RunRequest {
            root: dir.path(),
            kind: ValidationKind::UnitTest,
            tools: &WaitErrorRunner,
            subject: &allow(),
            task_id: None,
            context_pack_id: None,
            command_override: None,
            timeout: Duration::from_secs(1),
            max_output_bytes: 1024,
        })
        .unwrap();
    assert_eq!(result.outcome, Outcome::Unavailable);
    assert!(!result.passed);
    assert!(result.diagnostics[0].summary.contains("try_wait"));
}

#[test]
fn explicit_provider_override_beats_detection() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname=\"c\"\nversion=\"0.1.0\"\n",
    )
    .unwrap();
    fs::create_dir_all(dir.path().join(".agentos")).unwrap();
    fs::write(
        dir.path().join(".agentos/config.toml"),
        "[validation]\nproviders = [\"pytest\"]\n",
    )
    .unwrap();
    let config = load_project_config(&dir.path().join(".agentos/config.toml")).unwrap();
    let detected = detect(dir.path(), &config).unwrap();
    assert_eq!(detected.len(), 1);
    assert_eq!(detected[0].id, "pytest");
    let auto = detect(dir.path(), &ProjectConfig::default()).unwrap();
    assert_eq!(
        auto.iter().map(|item| item.id.as_str()).collect::<Vec<_>>(),
        ["cargo"]
    );
    fs::write(
        dir.path().join(".agentos/config.toml"),
        "[validation]\nproviders = [\"nope\"]\n",
    )
    .unwrap();
    let err = detect(
        dir.path(),
        &load_project_config(&dir.path().join(".agentos/config.toml")).unwrap(),
    )
    .unwrap_err();
    assert!(err.to_string().contains("unknown provider"));
    let empty = tempdir().unwrap();
    fs::write(empty.path().join("README.md"), "nothing\n").unwrap();
    assert!(
        detect(empty.path(), &ProjectConfig::default())
            .unwrap()
            .is_empty()
    );
    let _ = ValidationConfig::default();
}

#[test]
fn architecture_detectors_have_positive_and_negative_cases() {
    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    nodes.push(module("software.left.module", "left", "app/cycle/left.py"));
    nodes.push(module(
        "software.right.module",
        "right",
        "app/cycle/right.py",
    ));
    edges.push(imports(
        "software.left.module",
        "software.right.module",
        "app/cycle/left.py",
    ));
    edges.push(imports(
        "software.right.module",
        "software.left.module",
        "app/cycle/right.py",
    ));
    nodes.push(class(
        "software.orders.controller.class.OrderController",
        "OrderController",
        "app.orders.controller",
        "app/orders/controller.py",
    ));
    for name in ["a", "b", "c", "d", "e", "f"] {
        nodes.push(method(
            &format!("software.orders.controller.method.OrderController.{name}"),
            name,
            "OrderController",
            "app.orders.controller",
            "app/orders/controller.py",
        ));
    }
    nodes.push(import_node(
        "software.orders.domain.model.import.sqlalchemy",
        "sqlalchemy",
        "app/orders/domain/model.py",
    ));
    nodes.push(term(
        "domain.billing.terminology.Invoice",
        "Invoice",
        "docs/spec/requirements.md",
    ));
    nodes.push(term(
        "domain.billing.terminology.Bill",
        "Bill",
        "docs/spec/requirements.md",
    ));
    edges.push(conflicts(
        "domain.billing.terminology.Invoice",
        "domain.billing.terminology.Bill",
        "docs/spec/requirements.md",
    ));
    nodes.push(class(
        "software.billing.domain.model.class.UntestedCharge",
        "UntestedCharge",
        "app.billing.domain.model",
        "app/billing/domain/model.py",
    ));
    nodes.push(domain_entity(
        "domain.billing.entity.UntestedCharge",
        "UntestedCharge",
        "software.billing.domain.model.class.UntestedCharge",
        "app/billing/domain/model.py",
    ));
    nodes.push(class(
        "software.billing.passthrough.class.Forwarder",
        "Forwarder",
        "app.billing.passthrough",
        "app/billing/passthrough.py",
    ));
    nodes.push(method(
        "software.billing.passthrough.method.Forwarder.forward",
        "forward",
        "Forwarder",
        "app.billing.passthrough",
        "app/billing/passthrough.py",
    ));
    nodes.push(module(
        "software.orders.domain.model.module",
        "model",
        "app/orders/domain/model.py",
    ));
    nodes.push(module(
        "software.orders.adapter.db.module",
        "db",
        "app/orders/adapter/db.py",
    ));
    edges.push(imports(
        "software.orders.domain.model.module",
        "software.orders.adapter.db.module",
        "app/orders/domain/model.py",
    ));
    nodes.push(module(
        "software.orders.application.checkout.module",
        "checkout",
        "app/orders/application/checkout.py",
    ));
    edges.push(imports(
        "software.orders.application.checkout.module",
        "software.orders.adapter.db.module",
        "app/orders/application/checkout.py",
    ));
    for index in 0..7 {
        nodes.push(class(
            &format!("software.billing.surface.class.Widget{index}"),
            &format!("Widget{index}"),
            "app.billing.surface",
            "app/billing/surface.py",
        ));
    }
    let report = analyze(
        &GraphSnapshot {
            nodes: &nodes,
            edges: &edges,
            fragments: &[],
        },
        &ArchitectureConfig::default(),
    );
    let detectors: Vec<_> = report
        .findings
        .iter()
        .map(|item| item.detector.as_str())
        .collect();
    for expected in [
        "circular_dependency",
        "fat_controller",
        "domain_infrastructure",
        "duplicate_terminology",
        "missing_tests",
        "shallow_module",
        "boundary_leakage",
        "unwanted_direction",
        "oversized_surface",
    ] {
        assert!(
            detectors.contains(&expected),
            "missing {expected} in {detectors:?}"
        );
    }

    let thin = analyze(
        &GraphSnapshot {
            nodes: &[
                class(
                    "software.catalog.controller.class.CatalogController",
                    "CatalogController",
                    "app.catalog.controller",
                    "app/catalog/controller.py",
                ),
                method(
                    "software.catalog.controller.method.CatalogController.show",
                    "show",
                    "CatalogController",
                    "app.catalog.controller",
                    "app/catalog/controller.py",
                ),
                method(
                    "software.catalog.controller.method.CatalogController.update",
                    "update",
                    "CatalogController",
                    "app.catalog.controller",
                    "app/catalog/controller.py",
                ),
            ],
            edges: &[],
            fragments: &[],
        },
        &ArchitectureConfig::default(),
    );
    assert!(thin.findings.is_empty(), "{:?}", thin.findings);
}

#[test]
fn adr_exception_suppresses_domain_infrastructure() {
    let mut node = import_node(
        "software.orders.domain.model.import.sqlalchemy",
        "sqlalchemy",
        "app/orders/domain/model.py",
    );
    let decision = document_decision();
    let fragment = KnowledgeFragment {
        id: "decision.docs.adr.0001#0".into(),
        node_id: decision.id.clone(),
        source_ref: "docs/adr/0001.md".into(),
        location: "L1-L2".into(),
        content: "sqlalchemy is an allowed exception inside the order domain.\n".into(),
        fingerprint: "fp".into(),
        token_estimate: 1,
        kind: "section".into(),
    };
    let _ = &mut node;
    let report = analyze(
        &GraphSnapshot {
            nodes: &[node, decision],
            edges: &[],
            fragments: &[fragment],
        },
        &ArchitectureConfig::default(),
    );
    assert!(
        report
            .findings
            .iter()
            .all(|item| item.detector != "domain_infrastructure")
    );
}

fn provenance(source: &str) -> Provenance {
    Provenance {
        kind: ProvenanceKind::Observed,
        source: source.into(),
        location: "L1-L1".into(),
        extractor: "test".into(),
        extractor_version: "1".into(),
        method: "test".into(),
    }
}

fn base(id: &str, kind: NodeKind, name: &str, scope: &str, source: &str) -> KnowledgeNode {
    KnowledgeNode {
        id: id.into(),
        node_type: kind,
        name: name.into(),
        scope: scope.into(),
        summary: String::new(),
        status: "active".into(),
        provenance: provenance(source),
        confidence_millis: 900,
        version: 1,
        fingerprint: "fp".into(),
        source_refs: vec![source.into()],
        retrieval_metadata: BTreeMap::new(),
        payload: BTreeMap::new(),
        source_id: source.into(),
        parent: None,
    }
}

fn module(id: &str, name: &str, source: &str) -> KnowledgeNode {
    base(id, NodeKind::Module, name, name, source)
}

fn class(id: &str, name: &str, scope: &str, source: &str) -> KnowledgeNode {
    base(id, NodeKind::Class, name, scope, source)
}

fn method(id: &str, name: &str, parent: &str, scope: &str, source: &str) -> KnowledgeNode {
    let mut node = base(id, NodeKind::Method, name, scope, source);
    node.parent = Some(parent.into());
    node
}

fn import_node(id: &str, target: &str, source: &str) -> KnowledgeNode {
    let mut node = base(id, NodeKind::Import, target, "", source);
    node.payload.insert("target".into(), target.into());
    node
}

fn term(id: &str, name: &str, source: &str) -> KnowledgeNode {
    base(id, NodeKind::Terminology, name, "billing", source)
}

fn domain_entity(id: &str, name: &str, software: &str, source: &str) -> KnowledgeNode {
    let mut node = base(id, NodeKind::Entity, name, "billing", source);
    node.payload.insert("software_id".into(), software.into());
    node
}

fn document_decision() -> KnowledgeNode {
    base(
        "decision.docs.adr.0001",
        NodeKind::Decision,
        "0001",
        "docs.adr.0001",
        "docs/adr/0001.md",
    )
}

fn edge(from: &str, relation: RelationType, to: &str, source: &str) -> KnowledgeEdge {
    KnowledgeEdge {
        id: format!("{from}|{}|{to}", relation.as_str()),
        from_id: from.into(),
        relation,
        to_id: to.into(),
        confidence_millis: 900,
        provenance: provenance(source),
        metadata: BTreeMap::new(),
        source_id: source.into(),
    }
}

fn imports(from: &str, to: &str, source: &str) -> KnowledgeEdge {
    edge(from, RelationType::Imports, to, source)
}

fn conflicts(from: &str, to: &str, source: &str) -> KnowledgeEdge {
    edge(from, RelationType::ConflictsWith, to, source)
}

#[test]
fn workspace_fixtures_detect_expected_providers() {
    let root = workspace_root();
    let config = ProjectConfig::default();
    let ids = |fixture: &str| {
        detect(&root.join("fixtures").join(fixture), &config)
            .unwrap()
            .into_iter()
            .map(|item| item.id)
            .collect::<Vec<_>>()
    };
    assert_eq!(ids("cargo-sample"), ["cargo"]);
    assert_eq!(ids("pytest-fail"), ["pytest"]);
    assert_eq!(ids("go-sample"), ["go"]);
    assert!(ids("detect-none").is_empty());
    assert_eq!(
        ids("detect-multi"),
        ["go", "gradle", "jest", "maven", "rspec"]
    );
    assert_eq!(ids("python-fastapi"), ["pytest"]);
    assert_eq!(ids("typescript-react"), ["browser"]);
}

#[test]
fn cargo_and_go_samples_run_end_to_end() {
    let root = workspace_root();
    let subject = allow();
    let cargo_dir = copy_fixture(&root.join("fixtures/cargo-sample"));
    let cargo = run_and_store(
        cargo_dir.path(),
        ValidationKind::UnitTest,
        &ProcessRunner,
        &subject,
        None,
        None,
    )
    .unwrap();
    assert_eq!(cargo.len(), 1);
    assert!(cargo[0].passed, "{:?}", cargo[0].diagnostics);
    assert_eq!(cargo[0].provider, "cargo");
    assert_eq!(cargo[0].exit_status, Some(0));
    let go_dir = copy_fixture(&root.join("fixtures/go-sample"));
    let go = run_and_store(
        go_dir.path(),
        ValidationKind::UnitTest,
        &ProcessRunner,
        &subject,
        None,
        None,
    )
    .unwrap();
    assert!(go[0].passed, "{:?}", go[0].diagnostics);
    let fail_dir = copy_fixture(&root.join("fixtures/cargo-fail"));
    let failing = run_and_store(
        fail_dir.path(),
        ValidationKind::UnitTest,
        &ProcessRunner,
        &subject,
        Some("task-amount".into()),
        Some("pack-amount".into()),
    )
    .unwrap();
    assert!(!failing[0].passed);
    let item = failing[0]
        .feedback
        .iter()
        .find(|item| item.file.as_deref() == Some("src/lib.rs"))
        .unwrap_or_else(|| panic!("{:?}", failing[0].diagnostics));
    assert_eq!(item.line, Some(5));
    assert!(item.summary.contains("amount > 0"));
    assert_eq!(item.task_id.as_deref(), Some("task-amount"));
    assert_eq!(item.context_pack_id.as_deref(), Some("pack-amount"));
}

fn copy_fixture(from: &std::path::Path) -> tempfile::TempDir {
    let dir = tempdir().unwrap();
    copy_tree(from, dir.path());
    dir
}

fn copy_tree(from: &std::path::Path, to: &std::path::Path) {
    fs::create_dir_all(to).unwrap();
    let mut entries: Vec<_> = fs::read_dir(from)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let name = entry.file_name();
        if name == ".agentos" || name == "target" || name == ".venv" || name == "node_modules" {
            continue;
        }
        let target = to.join(&name);
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else if entry.file_type().unwrap().is_file() {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}
