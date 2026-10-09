//! End-to-end tests of the `agentos` binary.

use agentos_testkit::{CliRunner, TempProject};
use predicates::prelude::*;
use serde_json::Value;

fn runner() -> CliRunner {
    CliRunner::new(env!("CARGO_BIN_EXE_agentos"))
}

fn json_of(output: &[u8]) -> Value {
    serde_json::from_slice(output).unwrap_or_else(|e| {
        panic!(
            "stdout is not JSON ({e}): {}",
            String::from_utf8_lossy(output)
        )
    })
}

fn run_json(project: &TempProject, args: &[&str]) -> (i32, Value) {
    let out = runner()
        .command(project.path())
        .args(args)
        .arg("--json")
        .output()
        .unwrap();
    (out.status.code().unwrap(), json_of(&out.stdout))
}

fn entry_statuses(report: &Value) -> Vec<(String, String)> {
    report["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| {
            (
                e["path"].as_str().unwrap().to_owned(),
                e["status"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

fn check<'a>(report: &'a Value, name: &str) -> &'a Value {
    report["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == name)
        .unwrap_or_else(|| panic!("no check {name}"))
}

#[test]
fn version_flag() {
    let p = TempProject::new();
    runner()
        .command(p.path())
        .arg("--version")
        .assert()
        .success()
        .stdout(format!("agentos {}\n", env!("CARGO_PKG_VERSION")));
}

#[test]
fn usage_error_exits_2() {
    let p = TempProject::new();
    runner()
        .command(p.path())
        .arg("no-such-command")
        .assert()
        .code(2);
}

#[test]
fn init_creates_project_state_in_git_repo() {
    let p = TempProject::new().with_git();
    let (code, report) = run_json(&p, &["init"]);
    assert_eq!(code, 0);
    assert_eq!(report["root"], p.path().display().to_string());
    assert_eq!(report["root_source"], "git");
    assert_eq!(report["schema_version"], 1);
    assert_eq!(report["migrations_applied"], serde_json::json!([1]));
    assert_eq!(
        entry_statuses(&report),
        [
            (".agentos", "created"),
            (".agentos/config.toml", "created"),
            (".agentos/.gitignore", "created"),
            (".agentos/agentos.db", "created"),
            (".agentos/project-profile.json", "created"),
        ]
        .map(|(a, b)| (a.to_owned(), b.to_owned()))
    );
    assert!(p.join(".agentos/agentos.db").is_file());
    assert_eq!(
        p.read(".agentos/config.toml"),
        agentos_config::DEFAULT_CONFIG_TOML.as_bytes()
    );
}

#[test]
fn init_is_idempotent_and_preserves_files() {
    let p = TempProject::new().with_git();
    assert_eq!(run_json(&p, &["init"]).0, 0);
    let config_before = p.read(".agentos/config.toml");
    let gitignore_before = p.read(".agentos/.gitignore");

    let (code, second) = run_json(&p, &["init"]);
    assert_eq!(code, 0);
    assert!(
        entry_statuses(&second).iter().all(|(_, s)| s == "exists"),
        "second init changed something: {second}"
    );
    assert_eq!(second["migrations_applied"], serde_json::json!([]));
    assert_eq!(second["schema_version"], 1);
    assert_eq!(p.read(".agentos/config.toml"), config_before);
    assert_eq!(p.read(".agentos/.gitignore"), gitignore_before);

    p.write(".agentos/config.toml", "[log]\nlevel = \"info\"\n");
    runner().command(p.path()).arg("init").assert().success();
    assert_eq!(p.read(".agentos/config.toml"), b"[log]\nlevel = \"info\"\n");

    let (code, doctor) = run_json(&p, &["doctor"]);
    assert_eq!(code, 0, "{doctor}");
}

#[test]
fn init_text_output_reports_already_initialized() {
    let p = TempProject::new();
    runner()
        .command(p.path())
        .arg("init")
        .assert()
        .success()
        .stdout(predicate::str::starts_with("Initialized AgentOS project"));
    runner()
        .command(p.path())
        .arg("init")
        .assert()
        .success()
        .stdout(predicate::str::starts_with(
            "AgentOS project already initialized",
        ))
        .stdout(predicate::str::contains("migrations applied: none"));
}

#[test]
fn init_without_git_uses_working_directory() {
    let p = TempProject::new();
    let (code, report) = run_json(&p, &["init"]);
    assert_eq!(code, 0);
    assert_eq!(report["root_source"], "cwd");
    assert!(p.join(".agentos/config.toml").is_file());
    assert_eq!(run_json(&p, &["init"]).0, 0);
}

#[test]
fn init_from_subdirectory_targets_git_root() {
    let p = TempProject::new().with_git().with_dir("a/b");
    let out = runner()
        .command(&p.join("a/b"))
        .args(["init", "--json"])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(json_of(&out.stdout)["root"], p.path().display().to_string());
    assert!(p.join(".agentos").is_dir());
    assert!(!p.join("a/b/.agentos").exists());
}

#[test]
fn root_override_wins_and_must_exist() {
    let target = TempProject::new();
    let elsewhere = TempProject::new().with_git();
    let out = runner()
        .command(elsewhere.path())
        .args(["--root", target.path().to_str().unwrap(), "init", "--json"])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(json_of(&out.stdout)["root_source"], "override");
    assert!(target.join(".agentos").is_dir());
    assert!(!elsewhere.join(".agentos").exists());

    runner()
        .command(elsewhere.path())
        .args(["--root", "does/not/exist", "doctor"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("does not exist"));
}

#[test]
fn doctor_passes_after_init_with_git() {
    let p = TempProject::new().with_git();
    runner().command(p.path()).arg("init").assert().success();
    let (code, report) = run_json(&p, &["doctor"]);
    assert_eq!(code, 0, "{report}");
    assert_eq!(report["ok"], true);
    let names: Vec<&str> = report["checks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        ["root", "git", "state_dir", "config", "database", "writable"]
    );
    assert_eq!(check(&report, "git")["status"], "pass");
    assert_eq!(check(&report, "database")["status"], "pass");
    assert!(
        check(&report, "database")["message"]
            .as_str()
            .unwrap()
            .contains("schema version 1")
    );

    runner()
        .command(p.path())
        .arg("doctor")
        .assert()
        .success()
        .stdout(predicate::str::contains("Result: OK"));
}

#[test]
fn doctor_without_git_reports_no_git_and_passes_after_init() {
    let p = TempProject::new();
    runner().command(p.path()).arg("init").assert().success();
    let (code, report) = run_json(&p, &["doctor"]);
    assert_eq!(code, 0, "{report}");
    assert_eq!(check(&report, "git")["status"], "info");
    assert_eq!(check(&report, "git")["message"], "not a git repository");
}

#[test]
fn doctor_fails_on_uninitialized_project() {
    let p = TempProject::new();
    let (code, report) = run_json(&p, &["doctor"]);
    assert_eq!(code, 3);
    assert_eq!(report["ok"], false);
    assert_eq!(check(&report, "state_dir")["status"], "fail");
    assert_eq!(check(&report, "database")["status"], "fail");
    assert_eq!(check(&report, "config")["status"], "pass");
    assert_eq!(check(&report, "git")["status"], "info");
    assert!(!p.join(".agentos").exists(), "doctor must not create state");
}

#[test]
fn doctor_fails_on_invalid_config_and_config_command_errors() {
    let p = TempProject::new();
    runner().command(p.path()).arg("init").assert().success();
    p.write(".agentos/config.toml", "[log]\nlevle = \"info\"\n");

    let (code, report) = run_json(&p, &["doctor"]);
    assert_eq!(code, 3);
    assert_eq!(check(&report, "config")["status"], "fail");

    runner()
        .command(p.path())
        .arg("config")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("invalid config file"));
}

#[test]
fn doctor_fails_on_corrupted_database() {
    let p = TempProject::new();
    runner().command(p.path()).arg("init").assert().success();
    p.write(".agentos/agentos.db", "not a database");
    let (code, report) = run_json(&p, &["doctor"]);
    assert_eq!(code, 3);
    assert_eq!(check(&report, "database")["status"], "fail");
}

#[test]
fn config_precedence_through_the_binary() {
    let p = TempProject::new();
    runner().command(p.path()).arg("init").assert().success();
    p.write(
        ".agentos/config.toml",
        "[log]\nlevel = \"info\"\n[output]\nformat = \"text\"\n",
    );

    let value = |env: Option<&str>, flag: Option<&str>| {
        let mut cmd = runner().command(p.path());
        if let Some(level) = env {
            cmd.env("AGENTOS_LOG", level);
        }
        cmd.args(["config", "--json"]);
        if let Some(level) = flag {
            cmd.args(["--log-level", level]);
        }
        let out = cmd.output().unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let report = json_of(&out.stdout);
        (
            report["values"]["log.level"]["value"]
                .as_str()
                .unwrap()
                .to_owned(),
            report["values"]["log.level"]["source"]
                .as_str()
                .unwrap()
                .to_owned(),
        )
    };

    assert_eq!(value(None, None), ("info".into(), "file".into()));
    assert_eq!(
        value(Some("debug"), None),
        ("debug".into(), "env:AGENTOS_LOG".into())
    );
    assert_eq!(
        value(Some("debug"), Some("error")),
        ("error".into(), "cli".into())
    );
}

#[test]
fn output_format_from_env_and_verbose_logs_stay_on_stderr() {
    let p = TempProject::new();
    runner().command(p.path()).arg("init").assert().success();
    let out = runner()
        .command(p.path())
        .env("AGENTOS_OUTPUT", "json")
        .args(["-vv", "doctor"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let report = json_of(&out.stdout);
    assert_eq!(report["ok"], true);
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("project root"),
        "expected debug log on stderr"
    );
}

#[test]
fn config_text_shows_sources() {
    let p = TempProject::new();
    runner()
        .command(p.path())
        .arg("config")
        .assert()
        .success()
        .stdout(predicate::str::contains("(not found)"))
        .stdout(predicate::str::contains(
            "log.level = \"warn\"  # source: default",
        ));
}
