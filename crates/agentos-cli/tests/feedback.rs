//! CLI coverage for validation and architecture analysis.

use agentos_testkit::{CliRunner, copy_fixture};
use serde_json::Value;

fn runner() -> CliRunner {
    CliRunner::new(env!("CARGO_BIN_EXE_agentos"))
}

fn json_of(output: &[u8]) -> Value {
    serde_json::from_slice(output).unwrap_or_else(|err| {
        panic!(
            "stdout is not JSON ({err}): {}",
            String::from_utf8_lossy(output)
        )
    })
}

#[test]
fn validate_detect_json_lists_cargo() {
    let project = copy_fixture("cargo-sample");
    let out = runner()
        .command(project.path())
        .args(["validate", "detect", "--json"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{:?}", out);
    let body = json_of(&out.stdout);
    let ids: Vec<&str> = body
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["cargo"]);
}

#[test]
fn unknown_validation_kind_is_a_usage_error() {
    let project = copy_fixture("detect-none");
    runner()
        .command(project.path())
        .args(["validate", "--kind", "no-such"])
        .assert()
        .code(2);
}

#[test]
fn validate_with_no_provider_fails_the_check() {
    let project = copy_fixture("detect-none");
    let out = runner()
        .command(project.path())
        .args(["validate", "--kind", "unit_test", "--json"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(3), "{:?}", out);
    let body = json_of(&out.stdout);
    assert_eq!(body["results"].as_array().unwrap().len(), 0);
    assert_eq!(body["message"], "no provider supports unit_test");
}

#[test]
fn validate_last_without_a_result_is_a_runtime_error() {
    let project = copy_fixture("detect-none");
    runner()
        .command(project.path())
        .args(["validate", "last"])
        .assert()
        .code(1);
}

#[test]
fn architecture_good_fixture_exits_success() {
    let project = copy_fixture("good-architecture");
    let out = runner()
        .command(project.path())
        .args(["architecture", "analyze", "--json"])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let body = json_of(&out.stdout);
    assert_eq!(body["findings"].as_array().unwrap().len(), 0);
}

#[test]
fn architecture_bad_fixture_exits_check_failed() {
    let project = copy_fixture("bad-architecture");
    let out = runner()
        .command(project.path())
        .args(["architecture", "analyze", "--json"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(3), "{:?}", out);
    let body = json_of(&out.stdout);
    assert_eq!(body["findings"].as_array().unwrap().len(), 9);
}
