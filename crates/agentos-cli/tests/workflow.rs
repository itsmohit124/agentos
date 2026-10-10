//! CLI coverage for planning and running a workflow.

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
fn plan_and_mock_run_round_trip() {
    let project = copy_fixture("detect-none");
    let root = project.path().to_str().unwrap();
    let planned = runner()
        .command(project.path())
        .args([
            "--root",
            root,
            "plan",
            "Add retry support to payment webhook processing",
            "--workflow",
            "feature",
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(planned.status.code(), Some(0), "{planned:?}");
    let plan = json_of(&planned.stdout);
    assert_eq!(plan["workflow"], "feature");
    assert_eq!(
        plan["task"],
        "Add retry support to payment webhook processing"
    );
    assert_eq!(plan["selection"]["source"], "explicit");
    let skipped = plan["skipped"]
        .as_array()
        .unwrap()
        .iter()
        .find(|step| step["id"] == "integration_test")
        .expect("integration_test skip");
    assert_eq!(
        skipped["reason"],
        "workflow does not require integration validation for this task type"
    );
    let plan_id = plan["id"].as_str().unwrap();

    let ran = runner()
        .command(project.path())
        .args(["--root", root, "run", plan_id, "--runner", "mock", "--json"])
        .output()
        .unwrap();
    assert_eq!(ran.status.code(), Some(0), "{ran:?}");
    let run = json_of(&ran.stdout);
    assert_eq!(run["state"], "completed");
    assert_eq!(run["plan_id"], plan_id);
    let run_id = run["id"].as_str().unwrap();
    assert!(
        project
            .path()
            .join(".agentos/runs")
            .join(run_id)
            .join("run.json")
            .is_file()
    );

    let status = runner()
        .command(project.path())
        .args(["--root", root, "run", "status", run_id, "--json"])
        .output()
        .unwrap();
    assert_eq!(status.status.code(), Some(0), "{status:?}");
    assert_eq!(json_of(&status.stdout)["state"], "completed");
}

#[test]
fn unknown_workflow_is_a_usage_error() {
    let project = copy_fixture("detect-none");
    let root = project.path().to_str().unwrap();
    let out = runner()
        .command(project.path())
        .args([
            "--root",
            root,
            "plan",
            "Add a report",
            "--workflow",
            "deploy",
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("unknown workflow"), "{stderr}");
}

#[test]
fn missing_plan_is_a_runtime_error() {
    let project = copy_fixture("detect-none");
    let root = project.path().to_str().unwrap();
    let out = runner()
        .command(project.path())
        .args(["--root", root, "run", "feature-missing", "--runner", "mock"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("was not found"), "{stderr}");
}
