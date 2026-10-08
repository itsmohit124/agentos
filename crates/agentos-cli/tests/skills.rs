//! End-to-end tests of `agentos skills` and `agentos evaluate`.

use std::path::{Path, PathBuf};

use agentos_testkit::{CliRunner, TempProject, copy_fixture, fixture_path};
use predicates::prelude::*;
use serde_json::Value;

const BUILTIN_NAMES: [&str; 10] = [
    "architecture-review",
    "code-review",
    "domain-modeling",
    "engineering-router",
    "grilling",
    "implementation",
    "improve-codebase-architecture",
    "interface-design",
    "research",
    "tdd",
];

/// `agentos` in `project`, with a user-level skills directory that does not exist (so the
/// developer's real `~/.agentos/skills` never leaks into tests).
fn agentos(project: &TempProject) -> assert_cmd::Command {
    let mut cmd = CliRunner::new(env!("CARGO_BIN_EXE_agentos")).command(project.path());
    cmd.env("AGENTOS_USER_SKILLS_DIR", project.join("no-user-skills"));
    cmd
}

fn json(project: &TempProject, args: &[&str]) -> (i32, Value) {
    let out = agentos(project).args(args).arg("--json").output().unwrap();
    let value = serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
        panic!(
            "stdout is not JSON ({e}): {}\nstderr: {}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )
    });
    (out.status.code().unwrap(), value)
}

fn names(list: &Value) -> Vec<String> {
    list["skills"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["name"].as_str().unwrap().to_owned())
        .collect()
}

fn skill<'a>(list: &'a Value, name: &str) -> &'a Value {
    list["skills"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == name)
        .unwrap_or_else(|| panic!("no skill {name} in {list}"))
}

fn codes(report: &Value) -> Vec<String> {
    let mut codes: Vec<String> = report["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["code"].as_str().unwrap().to_owned())
        .collect();
    codes.sort();
    codes.dedup();
    codes
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

#[test]
fn list_shows_the_builtin_skills_with_invocation_flags() {
    let p = TempProject::new();
    let (code, list) = json(&p, &["skills", "list"]);
    assert_eq!(code, 0);
    assert_eq!(names(&list), BUILTIN_NAMES);
    assert_eq!(list["diagnostics"], Value::Array(vec![]));
    let grilling = skill(&list, "grilling");
    assert_eq!(grilling["level"], "builtin");
    assert_eq!(grilling["user_invocable"], true);
    assert_eq!(grilling["model_invocable"], false);
    let tdd = skill(&list, "tdd");
    assert_eq!(tdd["user_invocable"], true);
    assert_eq!(tdd["model_invocable"], true);
    assert_eq!(skill(&list, "engineering-router")["kind"], "router");
}

#[test]
fn list_text_shows_the_invocation_column() {
    let p = TempProject::new();
    agentos(&p)
        .args(["skills", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Skills: 10"))
        .stdout(predicate::str::contains("INVOKE"))
        .stdout(
            predicate::str::is_match(r"grilling\s+0\.1\.0\s+builtin\s+skill\s+user\s").unwrap(),
        );
}

#[test]
fn project_and_user_skills_override_builtins_by_priority() {
    let p = copy_fixture("skills/override");
    let root = p.join("project");
    let out = agentos(&p)
        .arg("--root")
        .arg(&root)
        .args(["skills", "list", "--json", "--user-skills-dir"])
        .arg(p.join("user"))
        .arg("--builtin-skills-dir")
        .arg(p.join("builtin"))
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    let list: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(names(&list), ["base", "helper", "notes", "tdd"]);
    assert_eq!(skill(&list, "tdd")["level"], "project");
    assert_eq!(
        skill(&list, "tdd")["overrides"],
        serde_json::json!(["user", "builtin"])
    );
    assert_eq!(skill(&list, "helper")["level"], "user");
    assert_eq!(skill(&list, "notes")["level"], "agentos");
    assert_eq!(skill(&list, "base")["level"], "builtin");
}

#[test]
fn the_user_skills_dir_can_come_from_the_environment() {
    let p = copy_fixture("skills/override");
    let out = agentos(&p)
        .env("AGENTOS_USER_SKILLS_DIR", p.join("user"))
        .env("AGENTOS_BUILTIN_SKILLS_DIR", p.join("builtin"))
        .args(["skills", "list", "--json"])
        .output()
        .unwrap();
    let list: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(skill(&list, "tdd")["level"], "user");
    assert_eq!(
        skill(&list, "tdd")["overrides"],
        serde_json::json!(["builtin"])
    );
}

#[test]
fn inspect_shows_metadata_resolution_permissions_and_body() {
    let p = TempProject::new();
    agentos(&p)
        .args(["skills", "inspect", "tdd"])
        .assert()
        .success()
        .stdout(predicate::str::contains("tdd 0.1.0"))
        .stdout(predicate::str::contains("user-invocable:  yes"))
        .stdout(predicate::str::contains("model-invocable: yes"))
        .stdout(predicate::str::contains(
            "requires:        interface-design ^0.1",
        ))
        .stdout(predicate::str::contains(
            "resolved order:  interface-design -> tdd",
        ))
        .stdout(predicate::str::contains(
            "allow [fs.read, fs.write, shell]; deny [network, browser] (default deny)",
        ))
        .stdout(predicate::str::contains("## Procedure"));
}

#[test]
fn inspect_brief_does_not_print_the_body() {
    let p = TempProject::new();
    agentos(&p)
        .args(["skills", "inspect", "grilling", "--brief"])
        .assert()
        .success()
        .stdout(predicate::str::contains("model-invocable: no"))
        .stdout(predicate::str::contains("## Procedure").not());
}

#[test]
fn inspect_json_includes_resolution_and_permissions() {
    let p = TempProject::new();
    let (code, v) = json(&p, &["skills", "inspect", "improve-codebase-architecture"]);
    assert_eq!(code, 0);
    assert_eq!(
        v["resolved_order"],
        serde_json::json!(["architecture-review", "improve-codebase-architecture"])
    );
    assert_eq!(v["manifest"]["invocation"]["model_invocable"], false);
    assert_eq!(v["denied"], serde_json::json!(["network", "browser"]));
    assert!(v["body"].as_str().unwrap().contains("## Must not"));
}

#[test]
fn inspect_of_an_unknown_skill_fails() {
    let p = TempProject::new();
    agentos(&p)
        .args(["skills", "inspect", "nope"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("no skill named \"nope\""));
}

#[test]
fn listing_never_reads_skill_bodies() {
    // The body is not valid UTF-8: anything that reads it fails, anything that does not succeeds.
    let mut text = b"---\nname: lazy\ndescription: Body is never read when listing.\nversion: \"0.1.0\"\neval_cases:\n  - task: lazy\n---\n".to_vec();
    text.extend_from_slice(&[0xff, 0xfe, 0xfd, b'\n']);
    let p = TempProject::new().with_file(".agents/skills/lazy/SKILL.md", text);

    let (code, list) = json(&p, &["skills", "list"]);
    assert_eq!(code, 0);
    assert_eq!(skill(&list, "lazy")["level"], "project");
    agentos(&p)
        .args(["skills", "inspect", "lazy", "--brief"])
        .assert()
        .success();
    agentos(&p).args(["skills", "graph"]).assert().success();
    agentos(&p)
        .args(["skills", "inspect", "lazy"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("cannot load the body"));
    let (code, report) = json(&p, &["skills", "validate"]);
    assert_eq!(code, 3);
    assert!(codes(&report).contains(&"unreadable_body".to_owned()));
}

#[test]
fn validate_passes_for_the_builtin_skills() {
    let p = TempProject::new();
    agentos(&p)
        .args(["skills", "validate"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Validated 10 skill(s): 0 error(s), 0 warning(s)",
        ))
        .stdout(predicate::str::contains("Result: OK"));
}

#[test]
fn validate_fails_for_broken_builtins() {
    let p = TempProject::new();
    let builtin = fixture_path("skills/broken-builtin/builtin");
    let out = agentos(&p)
        .args(["skills", "validate", "--json", "--builtin-skills-dir"])
        .arg(&builtin)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(3));
    let report: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(report["ok"], false);
    assert_eq!(codes(&report), ["missing_eval_cases", "missing_section"]);
}

#[test]
fn validate_reports_a_dependency_cycle() {
    let p = copy_fixture("skills/cycle/project");
    let (code, report) = json(&p, &["skills", "validate"]);
    assert_eq!(code, 3);
    assert!(
        codes(&report).contains(&"dependency_cycle".to_owned()),
        "{report}"
    );
}

#[test]
fn validate_rejects_implicit_model_invocation() {
    let p = copy_fixture("skills/implicit-model/project");
    let (code, report) = json(&p, &["skills", "validate"]);
    assert_eq!(code, 3);
    assert!(
        codes(&report).contains(&"implicit_model_invocation".to_owned()),
        "{report}"
    );
}

#[test]
fn graph_is_deterministic_and_has_a_dot_form() {
    let p = TempProject::new();
    let run = |args: &[&str]| agentos(&p).args(args).output().unwrap().stdout;
    let text = run(&["skills", "graph"]);
    assert_eq!(text, run(&["skills", "graph"]));
    let text = String::from_utf8(text).unwrap();
    assert!(text.starts_with("Skill graph: 10 skills, "), "{text}");
    assert!(text.contains("  requires -> interface-design ^0.1"));

    let dot = run(&["skills", "graph", "--format", "dot"]);
    assert_eq!(dot, run(&["skills", "graph", "--format", "dot"]));
    let dot = String::from_utf8(dot).unwrap();
    assert!(dot.starts_with("digraph skills {"), "{dot}");
    assert!(dot.trim_end().ends_with('}'));

    let (code, graph) = json(&p, &["skills", "graph"]);
    assert_eq!(code, 0);
    assert_eq!(graph["nodes"].as_array().unwrap().len(), 10);

    agentos(&p)
        .args(["skills", "graph", "--format", "dot", "--json"])
        .assert()
        .code(1);
}

#[test]
fn install_copies_a_local_skill_and_refuses_to_overwrite() {
    let p = TempProject::new();
    let source = fixture_path("skills/manifests/valid-full/full");
    agentos(&p)
        .args(["skills", "install"])
        .arg(&source)
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Installed full 1.2.3-beta.1 to .agents/skills/full",
        ));
    assert!(p.join(".agents/skills/full/SKILL.md").is_file());
    assert!(p.join(".agents/skills/full/references/notes.md").is_file());

    let (_, list) = json(&p, &["skills", "list"]);
    assert_eq!(skill(&list, "full")["level"], "project");

    agentos(&p)
        .args(["skills", "install"])
        .arg(&source)
        .assert()
        .code(1)
        .stderr(predicate::str::contains("already exists"));
}

#[test]
fn install_rejects_an_invalid_skill_without_writing() {
    let p = TempProject::new();
    agentos(&p)
        .args(["skills", "install"])
        .arg(fixture_path("skills/manifests/unknown-field/typo"))
        .assert()
        .code(1)
        .stderr(predicate::str::contains("is not a valid skill"));
    assert!(!p.join(".agents").exists());
}

#[test]
fn install_from_a_git_url_is_a_clear_stub() {
    let p = TempProject::new();
    agentos(&p)
        .args(["skills", "install", "https://example.com/skills/tdd.git"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("not supported yet"));
    assert!(!p.join(".agents").exists());
}

#[test]
fn evaluate_skills_passes_on_the_committed_suite() {
    let p = TempProject::new();
    let root = workspace_root();
    let (code, report) = json(
        &p,
        &["--root", root.to_str().unwrap(), "evaluate", "skills"],
    );
    assert_eq!(code, 0, "{report}");
    assert_eq!(report["ok"], true);
    assert_eq!(report["failed"], 0);
    assert!(report["by_kind"]["route"]["total"].as_u64().unwrap() >= 15);
}

#[test]
fn evaluate_exits_3_when_a_case_fails() {
    let p = TempProject::new().with_file(
        "cases/route_x.json",
        r#"{"id":"route_x","kind":"route","description":"deliberately wrong","fixture":"builtin:skills","oracle":"top","task":"Write tests first","invoker":"user","expect_top":"research"}"#,
    );
    agentos(&p)
        .args(["evaluate", "skills", "--evals-dir"])
        .arg(p.join("cases"))
        .assert()
        .code(3)
        .stdout(predicate::str::contains("[FAIL]"))
        .stdout(predicate::str::contains("Result: FAILED (0/1 passed)"));
}

#[test]
fn evaluate_without_cases_is_an_error() {
    let p = TempProject::new();
    agentos(&p)
        .args(["evaluate", "skills"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("no evaluation cases found"));
}
