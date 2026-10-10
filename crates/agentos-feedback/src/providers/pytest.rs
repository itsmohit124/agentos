//! pytest.

use std::path::Path;

use crate::error::FeedbackError;
use crate::kind::ValidationKind;
use crate::parseutil::{argv, dir_has, find_location, has_file, push_diagnostic};
use crate::provider::{CommandPlan, RunRequest, ValidationProvider, execute};
use crate::result::{CapturedOutput, ValidationResult, assemble};

/// Runs pytest.
#[derive(Debug, Default, Clone, Copy)]
pub struct PytestProvider;

const CAPABILITIES: &[ValidationKind] =
    &[ValidationKind::UnitTest, ValidationKind::IntegrationTest];

impl PytestProvider {
    /// Argv for `kind`, or the override when one was configured.
    ///
    /// Uses `.venv/bin/python` when that file exists, so a project virtualenv is used on a
    /// Homebrew Python that does not allow a system-wide `pip install`. Otherwise the command
    /// is `python3`.
    pub fn command(
        root: &Path,
        kind: ValidationKind,
        override_cmd: Option<&str>,
    ) -> Result<Vec<String>, FeedbackError> {
        let venv = root.join(".venv/bin/python");
        let python = if venv.is_file() {
            venv.display().to_string()
        } else {
            "python3".to_owned()
        };
        let default: Vec<&str> = match kind {
            ValidationKind::UnitTest => {
                vec![
                    python.as_str(),
                    "-m",
                    "pytest",
                    "-q",
                    "--tb=short",
                    "--color=no",
                ]
            }
            ValidationKind::IntegrationTest => vec![
                python.as_str(),
                "-m",
                "pytest",
                "-q",
                "--tb=short",
                "--color=no",
                "-m",
                "integration",
            ],
            other => {
                return Err(FeedbackError::UnsupportedKind {
                    provider: "pytest".to_owned(),
                    kind: other,
                });
            }
        };
        argv(&default, override_cmd)
    }

    /// Parses pytest `file:line:` locations from a run that did not succeed.
    ///
    /// Exit status 0 is a pass. A source location printed by a passing run is not a failure.
    /// Timeouts, crashes, and nonzero exits still keep file and line when the tool printed them.
    pub fn parse(kind: ValidationKind, captured: &CapturedOutput) -> ValidationResult {
        let mut diagnostics = Vec::new();
        if !succeeded(captured) {
            for line in captured.stdout.lines().chain(captured.stderr.lines()) {
                let trimmed = line.trim();
                if let Some(located) = find_location(trimmed, &["py"]) {
                    let message = if located.message.is_empty() {
                        "pytest failure"
                    } else {
                        located.message.as_str()
                    };
                    push_diagnostic(
                        &mut diagnostics,
                        "pytest",
                        &located.file,
                        located.line,
                        message,
                    );
                }
            }
        }
        assemble("pytest", kind, captured, diagnostics)
    }

    /// True when the project looks like a pytest tree.
    pub fn applies(root: &Path) -> bool {
        has_file(root, "pytest.ini")
            || has_file(root, "tox.ini")
            || has_file(root, "conftest.py")
            || dir_has(root, "tests", |name| {
                name.starts_with("test_") && name.ends_with(".py")
            })
            || dir_has(root, ".", |name| {
                name.starts_with("test_") && name.ends_with(".py")
            })
            || (has_file(root, "pyproject.toml")
                && (root.join("tests").is_dir() || file_contains(root, "pyproject.toml", "pytest")))
    }
}

/// A passing pytest process: it exited 0 and was not killed, crashed, or missing.
fn succeeded(captured: &CapturedOutput) -> bool {
    captured.exit_code == Some(0)
        && !captured.timed_out
        && !captured.crashed
        && captured.spawn_error.is_none()
}

fn file_contains(root: &Path, name: &str, needle: &str) -> bool {
    std::fs::read_to_string(root.join(name))
        .ok()
        .is_some_and(|text| text.contains(needle))
}

impl ValidationProvider for PytestProvider {
    fn id(&self) -> &'static str {
        "pytest"
    }

    fn capabilities(&self) -> &'static [ValidationKind] {
        CAPABILITIES
    }

    fn detect(&self, root: &Path) -> bool {
        Self::applies(root)
    }

    fn run(&self, request: &RunRequest<'_>) -> Result<ValidationResult, FeedbackError> {
        let argv = Self::command(
            request.root,
            request.kind,
            request.command_override.as_deref(),
        )?;
        let kind = request.kind;
        let env = vec![("PYTHONPATH".to_owned(), request.root.display().to_string())];
        execute(
            "pytest",
            kind,
            CommandPlan { argv, env },
            request,
            |captured| Self::parse(kind, captured),
        )
    }
}
