//! CLI coverage for project memory.

use agentos_testkit::{CliRunner, copy_fixture};

fn runner() -> CliRunner {
    CliRunner::new(env!("CARGO_BIN_EXE_agentos"))
}

fn output(status: &std::process::Output) -> String {
    format!(
        "status {:?}\nstdout:\n{}\nstderr:\n{}",
        status.status.code(),
        String::from_utf8_lossy(&status.stdout),
        String::from_utf8_lossy(&status.stderr)
    )
}

#[test]
fn memory_commands_round_trip() {
    let project = copy_fixture("detect-none");
    let root = project.path().to_str().unwrap();
    let bin = runner();
    let added = bin
        .command(project.path())
        .args([
            "--root",
            root,
            "memory",
            "add",
            "--type",
            "lesson",
            "--name",
            "Idempotency",
            "--content",
            "missing idempotency on the payment webhook",
            "--task",
            "Add retry support to payment webhook processing",
            "--outcome",
            "failed",
            "--at",
            "1970-01-01T00:00:00Z",
        ])
        .output()
        .unwrap();
    assert_eq!(added.status.code(), Some(0), "{}", output(&added));
    let added_text = String::from_utf8_lossy(&added.stdout);
    assert!(added_text.contains("status: proposed"), "{added_text}");
    assert!(added_text.contains("canonical: false"), "{added_text}");
    let id = added_text
        .lines()
        .find_map(|line| line.strip_prefix("id: "))
        .unwrap()
        .to_owned();

    let listed = bin
        .command(project.path())
        .args(["--root", root, "memory", "list"])
        .output()
        .unwrap();
    assert_eq!(listed.status.code(), Some(0), "{}", output(&listed));
    assert!(String::from_utf8_lossy(&listed.stdout).contains(&id));

    let shown = bin
        .command(project.path())
        .args(["--root", root, "memory", "show", &id])
        .output()
        .unwrap();
    assert_eq!(shown.status.code(), Some(0), "{}", output(&shown));
    assert!(String::from_utf8_lossy(&shown.stdout).contains("missing idempotency"));

    let queried = bin
        .command(project.path())
        .args([
            "--root",
            root,
            "memory",
            "query",
            "Add idempotency to payment webhook processing",
        ])
        .output()
        .unwrap();
    assert_eq!(queried.status.code(), Some(0), "{}", output(&queried));
    let queried_text = String::from_utf8_lossy(&queried.stdout);
    assert!(
        queried_text.contains(&id) && queried_text.contains("previous_failure:"),
        "{queried_text}"
    );

    let explained = bin
        .command(project.path())
        .args([
            "--root",
            root,
            "context",
            "explain",
            "Add idempotency to payment webhook processing",
        ])
        .output()
        .unwrap();
    assert_eq!(explained.status.code(), Some(0), "{}", output(&explained));
    let explained_text = String::from_utf8_lossy(&explained.stdout);
    assert!(
        explained_text.contains("memory:") && explained_text.contains(&id),
        "{explained_text}"
    );

    let confirmed = bin
        .command(project.path())
        .args([
            "--root",
            root,
            "memory",
            "confirm",
            &id,
            "--at",
            "1970-01-01T00:00:01Z",
        ])
        .output()
        .unwrap();
    assert_eq!(confirmed.status.code(), Some(0), "{}", output(&confirmed));
    let confirmed_text = String::from_utf8_lossy(&confirmed.stdout);
    assert!(
        confirmed_text.contains("status: confirmed"),
        "{confirmed_text}"
    );
    assert!(
        confirmed_text.contains("canonical: false"),
        "{confirmed_text}"
    );

    let forgotten = bin
        .command(project.path())
        .args([
            "--root",
            root,
            "memory",
            "forget",
            &id,
            "--at",
            "1970-01-01T00:00:02Z",
        ])
        .output()
        .unwrap();
    assert_eq!(forgotten.status.code(), Some(0), "{}", output(&forgotten));
    let after = bin
        .command(project.path())
        .args([
            "--root",
            root,
            "memory",
            "query",
            "Add idempotency to payment webhook processing",
        ])
        .output()
        .unwrap();
    assert_eq!(after.status.code(), Some(0), "{}", output(&after));
    assert!(!String::from_utf8_lossy(&after.stdout).contains(&id));

    let stale = bin
        .command(project.path())
        .args([
            "--root",
            root,
            "memory",
            "add",
            "--type",
            "semantic",
            "--name",
            "Old fact",
            "--content",
            "payment webhook used to require a signature",
            "--task",
            "payment webhook",
            "--link",
            "source|app/payments/service.py|fp-old",
            "--at",
            "1970-01-01T00:00:03Z",
        ])
        .output()
        .unwrap();
    assert_eq!(stale.status.code(), Some(0), "{}", output(&stale));
    let pruned = bin
        .command(project.path())
        .args(["--root", root, "memory", "prune", "--stale"])
        .output()
        .unwrap();
    assert_eq!(pruned.status.code(), Some(0), "{}", output(&pruned));
    assert!(String::from_utf8_lossy(&pruned.stdout).contains("memory.semantic."));

    let usage = bin
        .command(project.path())
        .args(["--root", root, "memory", "prune"])
        .output()
        .unwrap();
    assert_eq!(usage.status.code(), Some(2), "{}", output(&usage));

    let missing = bin
        .command(project.path())
        .args(["--root", root, "memory", "show", "memory.lesson.missing"])
        .output()
        .unwrap();
    assert_eq!(missing.status.code(), Some(1), "{}", output(&missing));
}
