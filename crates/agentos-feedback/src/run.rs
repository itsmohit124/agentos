//! Detection, execution, the latest result, and regression cases.

use std::fs;
use std::path::{Path, PathBuf};

use agentos_permissions::PermissionSubject;
use agentos_tools::ToolRunner;
use serde::Serialize;

use crate::config::{ProjectConfig, load_project_config};
use crate::error::FeedbackError;
use crate::kind::ValidationKind;
use crate::provider::{RunRequest, ValidationProvider};
use crate::providers::{builtin_providers, provider_by_id};
use crate::result::{CapturedOutput, Outcome, ValidationResult};
use crate::store::{self, load_raw, store_result};

/// A provider selected for a project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DetectedProvider {
    /// Provider id.
    pub id: String,
    /// Capabilities, in the provider's fixed order.
    pub capabilities: Vec<ValidationKind>,
}

/// Selects providers for `root`. An explicit config list replaces auto-detection.
pub fn detect(root: &Path, config: &ProjectConfig) -> Result<Vec<DetectedProvider>, FeedbackError> {
    let providers = selected(root, config)?;
    let mut detected: Vec<DetectedProvider> = providers
        .iter()
        .map(|provider| DetectedProvider {
            id: provider.id().to_owned(),
            capabilities: provider.capabilities().to_vec(),
        })
        .collect();
    detected.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(detected)
}

/// Runs `kind` and stores each result. Raw output is capped and stored beside the result.
pub fn run_and_store(
    root: &Path,
    kind: ValidationKind,
    tools: &dyn ToolRunner,
    subject: &dyn PermissionSubject,
    task_id: Option<String>,
    context_pack_id: Option<String>,
) -> Result<Vec<ValidationResult>, FeedbackError> {
    let config = load_project_config(&root.join(".agentos/config.toml"))?;
    let providers = selected(root, &config)?;
    let mut stored = Vec::new();
    for provider in providers {
        if !provider.capabilities().contains(&kind) {
            continue;
        }
        let recording = RecordingRunner {
            inner: tools,
            last: std::cell::RefCell::new(None),
        };
        let override_cmd = config.validation.commands.get(provider.id()).cloned();
        let request = RunRequest {
            root,
            kind,
            tools: &recording,
            subject,
            task_id: task_id.clone(),
            context_pack_id: context_pack_id.clone(),
            command_override: override_cmd,
            timeout: config.validation.timeout,
            max_output_bytes: config.validation.max_output_bytes,
        };
        let result = provider.run(&request)?;
        let (stdout, stderr) = recording
            .last
            .borrow()
            .clone()
            .unwrap_or_else(|| (String::new(), String::new()));
        stored.push(store_result(root, &result, &stdout, &stderr)?);
    }
    Ok(stored)
}

/// Most recent stored result.
pub fn last(root: &Path) -> Result<ValidationResult, FeedbackError> {
    store::last_result(root)
}

/// Promotes a failed result into `evals_dir/<id>.json`.
pub fn record_regression(
    root: &Path,
    id: &str,
    evals_dir: &Path,
) -> Result<PathBuf, FeedbackError> {
    let result = store::load_result(root, id)?;
    if result.passed || result.outcome == Outcome::Passed {
        return Err(FeedbackError::NotAFailure(id.to_owned()));
    }
    if result.outcome == Outcome::Refused {
        return Err(FeedbackError::NotReplayable(id.to_owned()));
    }
    let (stdout, stderr) = load_raw(root, id).unwrap_or_else(|_| (String::new(), String::new()));
    let case = RegressionCase {
        id: id.to_owned(),
        kind: "regression".to_owned(),
        description: format!("Promoted from validation {id}"),
        fixture: root.display().to_string(),
        oracle: "recorded-output".to_owned(),
        provider: result.provider.clone(),
        validation_kind: result.kind,
        exit_code: result.exit_status,
        timed_out: result.outcome == Outcome::TimedOut,
        crashed: result.outcome == Outcome::Crashed,
        spawn_error: (result.outcome == Outcome::Unavailable).then(|| {
            result
                .diagnostics
                .first()
                .map(|item| item.summary.clone())
                .unwrap_or_default()
        }),
        stdout,
        stderr,
        expected_outcome: result.outcome.as_str().to_owned(),
        expected_diagnostics: result
            .diagnostics
            .iter()
            .map(|diagnostic| ExpectedDiagnostic {
                file: diagnostic.file.clone(),
                line: diagnostic.line,
                summary_contains: diagnostic.summary.clone(),
            })
            .collect(),
    };
    fs::create_dir_all(evals_dir)?;
    let path = evals_dir.join(format!("{id}.json"));
    let json = serde_json::to_string_pretty(&case)
        .map_err(|err| FeedbackError::Config(err.to_string()))?;
    fs::write(&path, format!("{json}\n"))?;
    Ok(path)
}

/// Checks a regression case by parsing its recorded output again.
pub fn check_regression_case(case: &RegressionCase) -> Result<(), String> {
    let Some(provider) = provider_by_id(&case.provider) else {
        return Err(format!("unknown provider {}", case.provider));
    };
    let captured = CapturedOutput {
        target: case.fixture.clone(),
        command: Vec::new(),
        exit_code: case.exit_code,
        stdout: case.stdout.clone(),
        stderr: case.stderr.clone(),
        duration_ms: 0,
        timed_out: case.timed_out,
        crashed: case.crashed,
        spawn_error: case.spawn_error.clone(),
        truncated: false,
        task_id: None,
        context_pack_id: None,
    };
    let parsed = reparse(provider.as_ref(), case.validation_kind, &captured)?;
    if parsed.outcome.as_str() != case.expected_outcome {
        return Err(format!(
            "outcome {} != {}",
            parsed.outcome.as_str(),
            case.expected_outcome
        ));
    }
    for expected in &case.expected_diagnostics {
        let found = parsed.diagnostics.iter().any(|diagnostic| {
            diagnostic.file == expected.file
                && diagnostic.line == expected.line
                && diagnostic.summary.contains(&expected.summary_contains)
        });
        if !found {
            return Err(format!(
                "missing diagnostic {}:{} {}",
                expected.file.as_deref().unwrap_or("-"),
                expected.line.unwrap_or(0),
                expected.summary_contains
            ));
        }
    }
    Ok(())
}

fn reparse(
    provider: &dyn ValidationProvider,
    kind: ValidationKind,
    captured: &CapturedOutput,
) -> Result<ValidationResult, String> {
    // Concrete parsers are reached through the provider's public parse methods via a second
    // dispatch on the id, so a regression case does not spawn a process.
    dispatch_parse(provider.id(), kind, captured)
}

fn dispatch_parse(
    id: &str,
    kind: ValidationKind,
    captured: &CapturedOutput,
) -> Result<ValidationResult, String> {
    use crate::providers::{
        CargoTestProvider, GoTestProvider, GradleProvider, JestProvider, MavenProvider,
        PytestProvider, RSpecProvider,
    };
    Ok(match id {
        "cargo" => CargoTestProvider::parse(kind, captured),
        "pytest" => PytestProvider::parse(kind, captured),
        "jest" => JestProvider::parse(kind, captured),
        "go" => GoTestProvider::parse(kind, captured),
        "maven" => MavenProvider::parse(kind, captured),
        "gradle" => GradleProvider::parse(kind, captured),
        "rspec" => RSpecProvider::parse(kind, captured),
        other => return Err(format!("unknown provider {other}")),
    })
}

/// A regression case written by [`record_regression`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegressionCase {
    /// File stem.
    pub id: String,
    /// Always `regression`.
    pub kind: String,
    /// What was promoted.
    pub description: String,
    /// Project the failure came from.
    pub fixture: String,
    /// Always `recorded-output`.
    pub oracle: String,
    /// Provider id.
    pub provider: String,
    /// Kind that failed.
    pub validation_kind: ValidationKind,
    /// Exit code. Null when the process did not exit normally.
    pub exit_code: Option<i32>,
    /// The run timed out.
    pub timed_out: bool,
    /// The process crashed.
    pub crashed: bool,
    /// Spawn error, when the outcome is unavailable.
    pub spawn_error: Option<String>,
    /// Capped stdout.
    pub stdout: String,
    /// Capped stderr.
    pub stderr: String,
    /// Outcome wire name.
    pub expected_outcome: String,
    /// Diagnostics that must still be produced.
    pub expected_diagnostics: Vec<ExpectedDiagnostic>,
}

/// One diagnostic a regression case still expects.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct ExpectedDiagnostic {
    /// File. Null when the original diagnostic had none.
    pub file: Option<String>,
    /// Line. Null when the original diagnostic had none.
    pub line: Option<u32>,
    /// Substring of the summary.
    pub summary_contains: String,
}

fn selected(
    root: &Path,
    config: &ProjectConfig,
) -> Result<Vec<Box<dyn ValidationProvider>>, FeedbackError> {
    let mut providers = match &config.validation.providers {
        Some(ids) => {
            let mut chosen = Vec::new();
            for id in ids {
                let Some(provider) = provider_by_id(id) else {
                    return Err(FeedbackError::UnknownProvider(id.clone()));
                };
                chosen.push(provider);
            }
            chosen
        }
        None => builtin_providers()
            .into_iter()
            .filter(|provider| provider.detect(root))
            .collect(),
    };
    providers.sort_by(|left, right| left.id().cmp(right.id()));
    providers.dedup_by(|left, right| left.id() == right.id());
    Ok(providers)
}

struct RecordingRunner<'a> {
    inner: &'a dyn ToolRunner,
    last: std::cell::RefCell<Option<(String, String)>>,
}

impl ToolRunner for RecordingRunner<'_> {
    fn execute(
        &self,
        subject: &dyn PermissionSubject,
        request: &agentos_tools::ExecRequest,
    ) -> Result<agentos_tools::ExecOutcome, agentos_tools::ToolError> {
        let outcome = self.inner.execute(subject, request)?;
        *self.last.borrow_mut() = Some((outcome.stdout.clone(), outcome.stderr.clone()));
        Ok(outcome)
    }
}
