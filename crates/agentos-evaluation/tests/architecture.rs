//! Runs the committed `evals/architecture` suite.

use std::path::{Path, PathBuf};

use agentos_evaluation::architecture;

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn architecture_suite_passes() {
    let root = workspace();
    let report = architecture::run(&root.join("evals/architecture"), &root.join("fixtures"))
        .expect("suite runs");
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
    assert_eq!(report.passed(), 2, "{report:?}");
}
