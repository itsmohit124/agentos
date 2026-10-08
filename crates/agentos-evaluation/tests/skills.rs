//! Runs the committed `evals/skills` suite against the committed fixtures and built-in skills.

use std::path::{Path, PathBuf};

use agentos_evaluation::{EvalError, skills};

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn every_committed_skill_case_passes() {
    let root = workspace();
    let report = skills::run(
        &root.join("evals/skills"),
        &root.join("fixtures"),
        agentos_skills::builtin::location(),
    )
    .expect("suite runs");
    let failures: Vec<String> = report
        .cases
        .iter()
        .filter(|c| !c.passed)
        .map(|c| format!("{}: {}", c.id, c.detail))
        .collect();
    assert!(
        failures.is_empty(),
        "failing cases:\n{}",
        failures.join("\n")
    );
    let kinds = report.by_kind();
    assert!(
        kinds["route"].total >= 15,
        "need at least 15 router golden cases"
    );
    for kind in [
        "manifest",
        "discovery",
        "resolve",
        "permission",
        "validate",
        "skill_eval",
    ] {
        assert!(kinds.contains_key(kind), "missing case kind {kind}");
    }
}

#[test]
fn the_report_is_deterministic() {
    let root = workspace();
    let run = || {
        skills::run(
            &root.join("evals/skills"),
            &root.join("fixtures"),
            agentos_skills::builtin::location(),
        )
        .expect("suite runs")
    };
    assert_eq!(run(), run());
}

#[test]
fn a_missing_cases_directory_is_an_error_not_a_pass() {
    let tmp = tempfile::tempdir().unwrap();
    let err = skills::run(
        &tmp.path().join("nope"),
        tmp.path(),
        agentos_skills::builtin::location(),
    )
    .unwrap_err();
    assert!(matches!(err, EvalError::NoCases(_)));
}

#[test]
fn a_case_whose_id_differs_from_its_file_name_is_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join("route_001.json"),
        r#"{"id":"other","kind":"skill_eval","description":"d","fixture":"builtin:skills","oracle":"o"}"#,
    )
    .unwrap();
    let err = skills::run(tmp.path(), tmp.path(), agentos_skills::builtin::location()).unwrap_err();
    assert!(matches!(err, EvalError::InvalidCase { .. }), "{err}");
}

#[test]
fn unknown_case_fields_are_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join("x.json"),
        r#"{"id":"x","kind":"skill_eval","description":"d","fixture":"builtin:skills","oracle":"o","expect_tpo":"tdd"}"#,
    )
    .unwrap();
    let err = skills::run(tmp.path(), tmp.path(), agentos_skills::builtin::location()).unwrap_err();
    assert!(matches!(err, EvalError::InvalidCase { .. }), "{err}");
}

#[test]
fn a_wrong_expectation_fails_the_case() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join("r.json"),
        r#"{"id":"r","kind":"route","description":"d","fixture":"builtin:skills","oracle":"o","task":"Write tests first","invoker":"user","expect_top":"research"}"#,
    )
    .unwrap();
    let report = skills::run(tmp.path(), tmp.path(), agentos_skills::builtin::location()).unwrap();
    assert!(!report.is_ok());
    assert_eq!(report.failed(), 1);
}
