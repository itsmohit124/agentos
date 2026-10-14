//! Golden workflow suite.

use std::path::PathBuf;

#[test]
fn golden_workflows_catch_a_bad_patch_and_accept_a_good_one() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let report =
        agentos_evaluation::e2e::run(&root.join("evals/end-to-end"), &root.join("fixtures"))
            .expect("e2e suite");
    assert!(report.is_ok(), "{:?}", report.cases);
    for case in &report.cases {
        let trace = case.trace.as_ref().expect("trace");
        assert!(!trace.inputs.is_empty(), "{}", case.id);
        assert!(!trace.selected.is_empty(), "{}", case.id);
        assert!(!trace.validation.is_empty(), "{}", case.id);
    }
}
