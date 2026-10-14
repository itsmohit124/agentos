//! Runs the committed `evals/adapters` suite.

use std::path::{Path, PathBuf};

use agentos_evaluation::adapters;

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn adapter_suite_passes() {
    let root = workspace();
    let report =
        adapters::run(&root.join("evals/adapters"), &root.join("fixtures")).expect("suite runs");
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
    assert_eq!(report.passed(), 12, "{report:?}");
}
