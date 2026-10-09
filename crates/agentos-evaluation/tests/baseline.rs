//! The context baseline is a file-token measurement. It does not build a Context Pack.

use std::fs;
use std::path::{Path, PathBuf};

use agentos_evaluation::baseline;

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn two_measurements_match() {
    let root = workspace();
    let fixture = root.join("fixtures/python-fastapi");
    let first = baseline::measure(&root, &fixture).expect("baseline");
    let second = baseline::measure(&root, &fixture).expect("baseline");
    assert_eq!(first, second);
    assert!(first.full_repository_tokens > 0);
    assert!(first.file_count > 0);
    assert!(first.cases.len() >= 12);
    let webhook = first
        .cases
        .iter()
        .find(|case| case.id == "payment_webhook_007")
        .expect("webhook case");
    assert!(webhook.relevant_file_tokens < webhook.full_repository_tokens);
    assert!(webhook.relevant_file_tokens > 0);
    let rendered = baseline::render(&first).expect("json");
    assert!(!rendered.contains("timestamp"));
    assert_eq!(rendered, baseline::render(&second).expect("json"));
}

#[test]
fn committed_baseline_matches_a_fresh_measurement() {
    let root = workspace();
    let fixture = root.join("fixtures/python-fastapi");
    let report = baseline::measure(&root, &fixture).expect("baseline");
    let rendered = baseline::render(&report).expect("json");
    let path = root.join("evals/baselines/context-baseline.json");
    let committed =
        fs::read_to_string(&path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()));
    assert_eq!(rendered, committed);
}
