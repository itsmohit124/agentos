//! Gradle. A checked-in `gradlew` is preferred. The command passes `--offline`.

use std::path::Path;

use crate::error::FeedbackError;
use crate::kind::ValidationKind;
use crate::parseutil::{argv, find_location, has_file, push_diagnostic};
use crate::provider::{CommandPlan, RunRequest, ValidationProvider, execute};
use crate::result::{CapturedOutput, ValidationResult, assemble};

/// Runs Gradle tests.
#[derive(Debug, Default, Clone, Copy)]
pub struct GradleProvider;

const CAPABILITIES: &[ValidationKind] = &[ValidationKind::UnitTest, ValidationKind::Build];

impl GradleProvider {
    /// Argv for `kind`, or the override when one was configured.
    pub fn command(
        root: &Path,
        kind: ValidationKind,
        override_cmd: Option<&str>,
    ) -> Result<Vec<String>, FeedbackError> {
        if override_cmd.is_some() {
            return argv(&[], override_cmd);
        }
        let program: &str = if has_file(root, "gradlew") {
            "./gradlew"
        } else {
            "gradle"
        };
        let goal = match kind {
            ValidationKind::UnitTest => "test",
            ValidationKind::Build => "assemble",
            other => {
                return Err(FeedbackError::UnsupportedKind {
                    provider: "gradle".to_owned(),
                    kind: other,
                });
            }
        };
        argv(&[program, "--offline", goal], None)
    }

    /// Parses Gradle `File.java:line` frames.
    pub fn parse(kind: ValidationKind, captured: &CapturedOutput) -> ValidationResult {
        let mut diagnostics = Vec::new();
        for line in captured.stdout.lines().chain(captured.stderr.lines()) {
            if let Some(located) = find_location(line.trim(), &["java", "kt"]) {
                let message = if located.message.is_empty() {
                    "gradle test failure"
                } else {
                    located.message.as_str()
                };
                push_diagnostic(
                    &mut diagnostics,
                    "gradle",
                    &located.file,
                    located.line,
                    message,
                );
            }
        }
        assemble("gradle", kind, captured, diagnostics)
    }

    /// True when a Gradle build file is present.
    pub fn applies(root: &Path) -> bool {
        has_file(root, "build.gradle")
            || has_file(root, "build.gradle.kts")
            || has_file(root, "settings.gradle")
            || has_file(root, "settings.gradle.kts")
    }
}

impl ValidationProvider for GradleProvider {
    fn id(&self) -> &'static str {
        "gradle"
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
        execute(
            "gradle",
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
