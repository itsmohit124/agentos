//! Knowledge evaluation against the committed fixtures.

use std::path::PathBuf;

use agentos_evaluation::knowledge;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn knowledge_suite_passes_and_is_deterministic() {
    let cases = root().join("evals/knowledge");
    let fixtures = root().join("fixtures");
    let first = knowledge::run(&cases, &fixtures).unwrap();
    let second = knowledge::run(&cases, &fixtures).unwrap();
    assert!(first.is_ok(), "{first:?}");
    assert_eq!(first, second);
    assert_eq!(first.passed(), 5);
    assert_eq!(first.by_kind()["profile"].passed, 4);
    assert_eq!(first.by_kind()["graph"].passed, 1);
}

#[test]
fn case_id_must_match_file_name() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("wrong.json"),
        r#"{"id":"other","kind":"profile","description":"x","fixture":"empty-project","oracle":"expected/profile.json"}"#,
    )
    .unwrap();
    let err = knowledge::run(dir.path(), &root().join("fixtures")).unwrap_err();
    let message = err.to_string();
    assert!(message.contains("must equal the file name"), "{message}");
}
