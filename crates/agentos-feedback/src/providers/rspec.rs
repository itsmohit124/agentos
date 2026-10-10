//! RSpec. Detection understands a Gemfile that mentions `rspec`, a `.rspec` file, or `spec/`.

use std::path::Path;

use crate::error::FeedbackError;
use crate::kind::ValidationKind;
use crate::parseutil::{argv, find_location, has_file, push_diagnostic};
use crate::provider::{CommandPlan, RunRequest, ValidationProvider, execute};
use crate::result::{CapturedOutput, ValidationResult, assemble};

/// Runs RSpec.
#[derive(Debug, Default, Clone, Copy)]
pub struct RSpecProvider;

const CAPABILITIES: &[ValidationKind] = &[ValidationKind::UnitTest];

impl RSpecProvider {
    /// Argv for unit tests, or the override when one was configured.
    pub fn command(root: &Path, override_cmd: Option<&str>) -> Result<Vec<String>, FeedbackError> {
        if override_cmd.is_some() {
            return argv(&[], override_cmd);
        }
        let default: &[&str] = if gemfile_mentions_rspec(root) {
            &["bundle", "exec", "rspec"]
        } else {
            &["rspec"]
        };
        argv(default, None)
    }

    /// Parses `# ./spec/file.rb:line:` frames.
    pub fn parse(kind: ValidationKind, captured: &CapturedOutput) -> ValidationResult {
        let mut diagnostics = Vec::new();
        for line in captured.stdout.lines().chain(captured.stderr.lines()) {
            if let Some(located) =
                find_location(line.trim().trim_start_matches('#').trim(), &["rb"])
            {
                let message = if located.message.is_empty() {
                    "rspec failure"
                } else {
                    located.message.as_str()
                };
                push_diagnostic(
                    &mut diagnostics,
                    "rspec",
                    &located.file,
                    located.line,
                    message,
                );
            }
        }
        assemble("rspec", kind, captured, diagnostics)
    }

    /// True when the project declares RSpec.
    pub fn applies(root: &Path) -> bool {
        has_file(root, ".rspec") || root.join("spec").is_dir() || gemfile_mentions_rspec(root)
    }
}

fn gemfile_mentions_rspec(root: &Path) -> bool {
    std::fs::read_to_string(root.join("Gemfile"))
        .ok()
        .is_some_and(|text| text.to_ascii_lowercase().contains("rspec"))
}

impl ValidationProvider for RSpecProvider {
    fn id(&self) -> &'static str {
        "rspec"
    }

    fn capabilities(&self) -> &'static [ValidationKind] {
        CAPABILITIES
    }

    fn detect(&self, root: &Path) -> bool {
        Self::applies(root)
    }

    fn run(&self, request: &RunRequest<'_>) -> Result<ValidationResult, FeedbackError> {
        if request.kind != ValidationKind::UnitTest && request.command_override.is_none() {
            return Err(FeedbackError::UnsupportedKind {
                provider: "rspec".to_owned(),
                kind: request.kind,
            });
        }
        let argv = Self::command(request.root, request.command_override.as_deref())?;
        let kind = request.kind;
        execute(
            "rspec",
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
