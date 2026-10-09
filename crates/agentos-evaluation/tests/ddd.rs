//! Runs the committed `evals/ddd` suite.

use std::path::{Path, PathBuf};

use agentos_evaluation::ddd;

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn ddd_suite_passes() {
    let root = workspace();
    let report = ddd::run(&root.join("evals/ddd"), &root.join("fixtures")).expect("suite runs");
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
    assert_eq!(report.passed(), 6, "{report:?}");
}
