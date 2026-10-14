//! `agentos cleanup` removes derived state and managed adapter blocks only.

use agentos_testkit::{CliRunner, TempProject};

fn runner() -> CliRunner {
    CliRunner::new(env!("CARGO_BIN_EXE_agentos"))
}

#[test]
fn cleanup_requires_confirmation_and_keeps_skill_seals() {
    let project = TempProject::new().with_file(
        ".agents/skills/demo/SKILL.md",
        "---\nname: demo\ndescription: user only\nversion: \"0.1.0\"\ndisable-model-invocation: true\n---\n# Demo\n",
    ).with_file(
        "agents/openai.yaml",
        "policy:\n  allow_implicit_invocation: false\n",
    ).with_file("keep.txt", "leave this\n");
    let root = project.path().to_str().unwrap();
    runner()
        .command(project.path())
        .args(["--root", root, "init"])
        .assert()
        .success();
    runner()
        .command(project.path())
        .args(["--root", root, "adapter", "install", "generic"])
        .assert()
        .success();
    runner()
        .command(project.path())
        .args(["--root", root, "adapter", "install", "cursor"])
        .assert()
        .success();
    assert!(project.path().join(".agentos").is_dir());
    assert!(project.path().join(".cursor/rules/agentos.mdc").is_file());

    runner()
        .command(project.path())
        .args(["--root", root, "cleanup"])
        .assert()
        .code(2);
    assert!(project.path().join(".agentos").is_dir());
    assert!(project.path().join(".cursor/rules/agentos.mdc").is_file());

    runner()
        .command(project.path())
        .args(["--root", root, "cleanup", "--yes"])
        .assert()
        .success();
    assert!(!project.path().join(".agentos").exists());
    assert!(!project.path().join(".cursor/rules/agentos.mdc").exists());
    let skill =
        std::fs::read_to_string(project.path().join(".agents/skills/demo/SKILL.md")).unwrap();
    assert!(skill.contains("disable-model-invocation: true"));
    let policy = std::fs::read_to_string(project.path().join("agents/openai.yaml")).unwrap();
    assert!(policy.contains("allow_implicit_invocation: false"));
    assert_eq!(
        std::fs::read_to_string(project.path().join("keep.txt")).unwrap(),
        "leave this\n"
    );
}
