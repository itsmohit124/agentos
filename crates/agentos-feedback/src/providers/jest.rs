//! Jest. The command prefers `npm test` when the test script invokes Jest, and otherwise
//! `npx --no-install jest` so a missing install does not reach the network.

use std::path::Path;

use crate::error::FeedbackError;
use crate::kind::ValidationKind;
use crate::parseutil::{argv, find_location, has_file, push_diagnostic};
use crate::provider::{CommandPlan, RunRequest, ValidationProvider, execute};
use crate::result::{CapturedOutput, ValidationResult, assemble};

/// Runs Jest.
#[derive(Debug, Default, Clone, Copy)]
pub struct JestProvider;

const CAPABILITIES: &[ValidationKind] = &[ValidationKind::UnitTest];

impl JestProvider {
    /// Argv for unit tests, or the override when one was configured.
    pub fn command(root: &Path, override_cmd: Option<&str>) -> Result<Vec<String>, FeedbackError> {
        if override_cmd.is_some() {
            return argv(&[], override_cmd);
        }
        let script = test_script(root);
        let default: &[&str] = if script.as_deref().is_some_and(script_invokes_jest) {
            &[
                "npm",
                "test",
                "--silent",
                "--",
                "--watchAll=false",
                "--colors=false",
            ]
        } else {
            &[
                "npx",
                "--no-install",
                "jest",
                "--watchAll=false",
                "--colors=false",
            ]
        };
        argv(default, None)
    }

    /// Parses Jest stack frames `(file:line:column)`.
    pub fn parse(kind: ValidationKind, captured: &CapturedOutput) -> ValidationResult {
        let mut diagnostics = Vec::new();
        let text = format!("{}\n{}", captured.stdout, captured.stderr);
        let lines: Vec<&str> = text.lines().collect();
        for (index, line) in lines.iter().enumerate() {
            if let Some(located) = find_location(line, &["js", "jsx", "ts", "tsx"]) {
                let message = nearby_message(&lines, index).unwrap_or(located.message);
                push_diagnostic(
                    &mut diagnostics,
                    "jest",
                    &located.file,
                    located.line,
                    &message,
                );
            }
        }
        assemble("jest", kind, captured, diagnostics)
    }

    /// True when `package.json` declares Jest as a dependency or a test script.
    pub fn applies(root: &Path) -> bool {
        has_file(root, "package.json") && package_uses_jest(root)
    }
}

fn package_uses_jest(root: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(root.join("package.json")) else {
        return false;
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
        return false;
    };
    for key in ["dependencies", "devDependencies", "peerDependencies"] {
        if value.get(key).and_then(|entry| entry.get("jest")).is_some() {
            return true;
        }
    }
    test_script_value(&value).is_some_and(|script| script_invokes_jest(&script))
}

fn test_script(root: &Path) -> Option<String> {
    let text = std::fs::read_to_string(root.join("package.json")).ok()?;
    let value = serde_json::from_str::<serde_json::Value>(&text).ok()?;
    test_script_value(&value)
}

fn test_script_value(value: &serde_json::Value) -> Option<String> {
    value
        .get("scripts")
        .and_then(|scripts| scripts.get("test"))
        .and_then(|script| script.as_str())
        .map(str::to_owned)
}

fn script_invokes_jest(script: &str) -> bool {
    script
        .split_whitespace()
        .any(|word| word == "jest" || word.ends_with("/jest") || word.ends_with("\\jest"))
}

fn nearby_message(lines: &[&str], index: usize) -> Option<String> {
    let start = index.saturating_sub(6);
    let window = &lines[start..index];
    if let Some(bullet) = window.iter().rev().find_map(|line| {
        let trimmed = line.trim();
        trimmed
            .strip_prefix('●')
            .map(|rest| rest.trim().to_owned())
            .filter(|rest| !rest.is_empty())
    }) {
        return Some(bullet);
    }
    window.iter().rev().find_map(|line| {
        let trimmed = line.trim();
        if trimmed.contains("Error") || trimmed.contains("expect") {
            Some(trimmed.to_owned())
        } else {
            None
        }
    })
}

impl ValidationProvider for JestProvider {
    fn id(&self) -> &'static str {
        "jest"
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
                provider: "jest".to_owned(),
                kind: request.kind,
            });
        }
        let argv = Self::command(request.root, request.command_override.as_deref())?;
        let kind = request.kind;
        execute(
            "jest",
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
