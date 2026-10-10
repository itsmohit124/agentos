//! Cargo: test, check, build, and clippy.

use std::path::Path;

use crate::error::FeedbackError;
use crate::kind::ValidationKind;
use crate::parseutil::{argv, find_location, has_file, push_diagnostic};
use crate::provider::{CommandPlan, RunRequest, ValidationProvider, execute};
use crate::result::{CapturedOutput, Diagnostic, ValidationResult, assemble, one_line};

/// Runs Cargo for Rust projects.
#[derive(Debug, Default, Clone, Copy)]
pub struct CargoTestProvider;

const CAPABILITIES: &[ValidationKind] = &[
    ValidationKind::UnitTest,
    ValidationKind::Typecheck,
    ValidationKind::Lint,
    ValidationKind::Build,
];

impl CargoTestProvider {
    /// Argv for `kind`, or the override when one was configured.
    pub fn command(
        kind: ValidationKind,
        override_cmd: Option<&str>,
    ) -> Result<Vec<String>, FeedbackError> {
        let default: &[&str] = match kind {
            ValidationKind::UnitTest => &["cargo", "test", "--offline", "--color", "never"],
            ValidationKind::Typecheck => &["cargo", "check", "--offline", "--color", "never"],
            ValidationKind::Lint => &[
                "cargo",
                "clippy",
                "--offline",
                "--color",
                "never",
                "--",
                "-D",
                "warnings",
            ],
            ValidationKind::Build => &["cargo", "build", "--offline", "--color", "never"],
            other => {
                return Err(FeedbackError::UnsupportedKind {
                    provider: "cargo".to_owned(),
                    kind: other,
                });
            }
        };
        argv(default, override_cmd)
    }

    /// Parses Cargo test and rustc output.
    pub fn parse(kind: ValidationKind, captured: &CapturedOutput) -> ValidationResult {
        let mut diagnostics = Vec::new();
        let lines: Vec<&str> = captured
            .stdout
            .lines()
            .chain(captured.stderr.lines())
            .collect();
        for (index, line) in lines.iter().enumerate() {
            if let Some(rest) = line.split("panicked at ").nth(1)
                && let Some(located) = find_location(rest, &["rs"])
            {
                let message = lines
                    .get(index + 1)
                    .map(|next| next.trim())
                    .filter(|next| !next.is_empty())
                    .unwrap_or("panic");
                push_diagnostic(
                    &mut diagnostics,
                    "cargo",
                    &located.file,
                    located.line,
                    message,
                );
                continue;
            }
            if line.contains("-->")
                && let Some(located) = find_location(line, &["rs"])
            {
                let message = lines
                    .get(index.saturating_sub(1))
                    .map(|prev| prev.trim())
                    .filter(|prev| prev.starts_with("error"))
                    .unwrap_or("rustc error");
                push_diagnostic(
                    &mut diagnostics,
                    "cargo",
                    &located.file,
                    located.line,
                    message,
                );
            }
        }
        if diagnostics.is_empty()
            && captured.stdout.contains("test result: FAILED")
            && let Some(name) = failed_test_name(&captured.stdout)
        {
            diagnostics.push(Diagnostic {
                tool: "cargo".to_owned(),
                summary: one_line(&format!("test {name} failed")),
                file: None,
                line: None,
            });
        }
        assemble("cargo", kind, captured, diagnostics)
    }

    /// True when the project root has a `Cargo.toml`.
    pub fn applies(root: &Path) -> bool {
        has_file(root, "Cargo.toml")
    }
}

fn failed_test_name(stdout: &str) -> Option<String> {
    stdout.lines().find_map(|line| {
        let name = line
            .trim()
            .strip_prefix("test ")?
            .strip_suffix(" ... FAILED")?;
        Some(name.to_owned())
    })
}

impl ValidationProvider for CargoTestProvider {
    fn id(&self) -> &'static str {
        "cargo"
    }

    fn capabilities(&self) -> &'static [ValidationKind] {
        CAPABILITIES
    }

    fn detect(&self, root: &Path) -> bool {
        Self::applies(root)
    }

    fn run(&self, request: &RunRequest<'_>) -> Result<ValidationResult, FeedbackError> {
        let argv = Self::command(request.kind, request.command_override.as_deref())?;
        let kind = request.kind;
        execute(
            "cargo",
            kind,
            CommandPlan {
                argv,
                env: Vec::new(),
            },
            request,
            |captured| Self::parse(kind, captured),
        )
    }
}
