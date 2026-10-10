//! Maven. The constructed command is offline (`mvn -o`) so a run does not download plugins.

use std::path::Path;

use crate::error::FeedbackError;
use crate::kind::ValidationKind;
use crate::parseutil::{argv, find_location, has_file, push_diagnostic};
use crate::provider::{CommandPlan, RunRequest, ValidationProvider, execute};
use crate::result::{CapturedOutput, ValidationResult, assemble};

/// Runs Maven tests.
#[derive(Debug, Default, Clone, Copy)]
pub struct MavenProvider;

const CAPABILITIES: &[ValidationKind] = &[ValidationKind::UnitTest, ValidationKind::Build];

impl MavenProvider {
    /// Argv for `kind`, or the override when one was configured.
    pub fn command(
        kind: ValidationKind,
        override_cmd: Option<&str>,
    ) -> Result<Vec<String>, FeedbackError> {
        let default: &[&str] = match kind {
            ValidationKind::UnitTest => &["mvn", "-o", "-q", "test"],
            ValidationKind::Build => &["mvn", "-o", "-q", "-DskipTests", "package"],
            other => {
                return Err(FeedbackError::UnsupportedKind {
                    provider: "maven".to_owned(),
                    kind: other,
                });
            }
        };
        argv(default, override_cmd)
    }

    /// Parses Surefire `File.java:line` locations.
    pub fn parse(kind: ValidationKind, captured: &CapturedOutput) -> ValidationResult {
        let mut diagnostics = Vec::new();
        for line in captured.stdout.lines().chain(captured.stderr.lines()) {
            if let Some(located) = find_location(line.trim(), &["java"]) {
                let message = if located.message.is_empty() {
                    "maven test failure"
                } else {
                    located.message.as_str()
                };
                push_diagnostic(
                    &mut diagnostics,
                    "maven",
                    &located.file,
                    located.line,
                    message,
                );
            }
        }
        assemble("maven", kind, captured, diagnostics)
    }

    /// True when the project root has a `pom.xml`.
    pub fn applies(root: &Path) -> bool {
        has_file(root, "pom.xml")
    }
}

impl ValidationProvider for MavenProvider {
    fn id(&self) -> &'static str {
        "maven"
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
            "maven",
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
