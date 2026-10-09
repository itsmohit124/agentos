//! Runs the committed `evals/context` suite.

use std::path::{Path, PathBuf};

use agentos_evaluation::context;

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn context_suite_passes() {
    let root = workspace();
    let report =
        context::run(&root.join("evals/context"), &root.join("fixtures")).expect("suite runs");
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
    assert!(report.passed() >= 12, "{report:?}");
}
