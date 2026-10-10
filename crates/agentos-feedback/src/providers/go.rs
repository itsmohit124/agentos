//! `go test`. Real execution sets `GOPROXY=off` so the run does not download modules.

use std::path::Path;

use crate::error::FeedbackError;
use crate::kind::ValidationKind;
use crate::parseutil::{argv, find_location, has_file, push_diagnostic};
use crate::provider::{CommandPlan, RunRequest, ValidationProvider, execute};
use crate::result::{CapturedOutput, ValidationResult, assemble};

/// Runs `go test ./...`.
#[derive(Debug, Default, Clone, Copy)]
pub struct GoTestProvider;

const CAPABILITIES: &[ValidationKind] = &[ValidationKind::UnitTest, ValidationKind::Build];

impl GoTestProvider {
    /// Argv for `kind`, or the override when one was configured.
    pub fn command(
        kind: ValidationKind,
        override_cmd: Option<&str>,
    ) -> Result<Vec<String>, FeedbackError> {
        let default: &[&str] = match kind {
            ValidationKind::UnitTest => &["go", "test", "./..."],
            ValidationKind::Build => &["go", "test", "-c", "-o", "/dev/null", "./..."],
            other => {
                return Err(FeedbackError::UnsupportedKind {
                    provider: "go".to_owned(),
                    kind: other,
                });
            }
        };
        argv(default, override_cmd)
    }

    /// Parses `file.go:line: message` lines from `go test`.
    pub fn parse(kind: ValidationKind, captured: &CapturedOutput) -> ValidationResult {
        let mut diagnostics = Vec::new();
        for line in captured.stdout.lines().chain(captured.stderr.lines()) {
            if let Some(located) = find_location(line.trim(), &["go"]) {
                let message = if located.message.is_empty() {
                    "go test failure"
                } else {
                    located.message.as_str()
                };
                push_diagnostic(&mut diagnostics, "go", &located.file, located.line, message);
            }
        }
        assemble("go", kind, captured, diagnostics)
    }

    /// True when the project root has a `go.mod`.
    pub fn applies(root: &Path) -> bool {
        has_file(root, "go.mod")
    }
}

impl ValidationProvider for GoTestProvider {
    fn id(&self) -> &'static str {
        "go"
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
        let env = if request.command_override.is_some() {
            Vec::new()
        } else {
            vec![
                ("GOPROXY".to_owned(), "off".to_owned()),
                ("GOFLAGS".to_owned(), "-mod=readonly".to_owned()),
            ]
        };
        execute("go", kind, CommandPlan { argv, env }, request, |captured| {
            Self::parse(kind, captured)
        })
    }
}
