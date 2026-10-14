//! `agentos adapter` against a disposable project.

use agentos_testkit::{CliRunner, TempProject};
use predicates::prelude::*;

fn runner() -> CliRunner {
    CliRunner::new(env!("CARGO_BIN_EXE_agentos"))
}

fn project() -> TempProject {
    TempProject::new()
}

#[test]
fn list_does_not_write_and_names_every_adapter() {
    let project = project();
    let output = runner()
        .command(project.path())
        .args([
            "--root",
            project.path().to_str().unwrap(),
            "adapter",
            "list",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(output).unwrap();
    for name in ["claude", "codex", "cursor", "generic"] {
        assert!(text.contains(name), "{text}");
    }
    assert!(!project.path().join(".agentos").exists());
    assert!(!project.path().join("AGENTS.md").exists());
    assert!(!project.path().join(".cursor").exists());
}

#[test]
fn install_doctor_and_uninstall_use_the_project_root() {
    let project = project();
    let root = project.path().to_str().unwrap();
    runner()
        .command(project.path())
        .args(["--root", root, "adapter", "doctor", "generic"])
        .assert()
        .code(3);
    runner()
        .command(project.path())
        .args(["--root", root, "adapter", "install", "generic"])
        .assert()
        .success();
    assert!(
        project
            .path()
            .join(".agentos/adapters/generic/INSTRUCTIONS.md")
            .is_file()
    );
    runner()
        .command(project.path())
        .args(["--root", root, "adapter", "install", "generic"])
        .assert()
        .success()
        .stdout(predicate::str::contains("unchanged"));
    runner()
        .command(project.path())
        .args(["--root", root, "adapter", "doctor", "generic"])
        .assert()
        .success();
    runner()
        .command(project.path())
        .args(["--root", root, "adapter", "uninstall", "generic"])
        .assert()
        .success();
    assert!(
        !project
            .path()
            .join(".agentos/adapters/generic/INSTRUCTIONS.md")
            .exists()
    );
}

#[test]
fn unknown_adapter_is_a_runtime_error() {
    let project = project();
    runner()
        .command(project.path())
        .args([
            "--root",
            project.path().to_str().unwrap(),
            "adapter",
            "install",
            "nope",
        ])
        .assert()
        .code(1);
}
