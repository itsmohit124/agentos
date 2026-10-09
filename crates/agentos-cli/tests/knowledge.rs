//! CLI coverage for project understanding and the knowledge graph.

use agentos_testkit::{CliRunner, TempProject, copy_fixture};
use predicates::prelude::*;

fn runner() -> CliRunner {
    CliRunner::new(env!("CARGO_BIN_EXE_agentos"))
}

const PAYMENT_SERVICE: &str = "software.app.payments.service.class.PaymentService";

#[test]
fn profile_before_init_exits_1() {
    let project = TempProject::new();
    runner()
        .command(project.path())
        .args(["project", "profile"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("no project profile"));
}

#[test]
fn scan_is_incremental_and_inspect_finds_payment_service() {
    let project = copy_fixture("python-fastapi");
    runner()
        .command(project.path())
        .arg("init")
        .assert()
        .success();
    runner()
        .command(project.path())
        .args(["project", "profile", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"classification\": \"EXISTING\""))
        .stdout(predicate::str::contains("\"maturity\": \"MATURE\""));

    runner()
        .command(project.path())
        .arg("scan")
        .assert()
        .success();
    runner()
        .command(project.path())
        .arg("scan")
        .assert()
        .success()
        .stdout(predicate::str::contains("0 changed"))
        .stdout(predicate::str::contains("rows written 0"));

    runner()
        .command(project.path())
        .args(["knowledge", "stats"])
        .assert()
        .success()
        .stdout(predicate::str::contains("nodes "));
    runner()
        .command(project.path())
        .args(["knowledge", "inspect", PAYMENT_SERVICE])
        .assert()
        .success()
        .stdout(predicate::str::contains("CALLS"))
        .stdout(predicate::str::contains("PaymentRepository"));
    runner()
        .command(project.path())
        .args(["knowledge", "inspect", "missing.node"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("no knowledge node"));

    let db = project.path().join(".agentos/agentos.db");
    std::fs::remove_file(&db).unwrap();
    runner()
        .command(project.path())
        .arg("scan")
        .assert()
        .success()
        .stdout(predicate::str::contains("nodes "));
    runner()
        .command(project.path())
        .args(["knowledge", "inspect", PAYMENT_SERVICE, "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains(PAYMENT_SERVICE));
}

#[test]
fn empty_fixture_profile_is_uncertain() {
    let project = copy_fixture("empty-project");
    runner()
        .command(project.path())
        .arg("init")
        .assert()
        .success();
    runner()
        .command(project.path())
        .args(["project", "profile", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "\"classification\": \"GREENFIELD\"",
        ))
        .stdout(predicate::str::contains("\"maturity\": \"EMPTY\""))
        .stdout(predicate::str::contains("\"uncertain\": true"));
}
