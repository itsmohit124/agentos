//! Runs the committed `evals/workflow` suite.

use std::path::{Path, PathBuf};

use agentos_evaluation::workflow;

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn workflow_suite_passes() {
    let root = workspace();
    let report =
        workflow::run(&root.join("evals/workflow"), &root.join("fixtures")).expect("suite runs");
    let failures: Vec<String> = report
        .cases
        .iter()
        .filter(|case| !case.passed)
        .map(|case| format!("{}: {}", case.id, case.detail))
        .collect();
    assert!(
        failures.is_empty(),
        "failing cases:\n{}",
        failures.join("\n")
    );
    assert_eq!(report.passed(), 21, "{report:?}");
}
