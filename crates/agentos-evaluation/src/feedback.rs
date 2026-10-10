//! Feedback evaluation: detection, command construction, recorded-output parsing,
//! permission refusal, and regressions.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use agentos_core::fs::{FileSystem, RealFs};
use agentos_feedback::{
    CargoTestProvider, GoTestProvider, GradleProvider, JestProvider, MavenProvider, PytestProvider,
    RSpecProvider, ValidationKind, ValidationProvider, check_regression_case,
};
use agentos_permissions::{PermissionSet, PermissionSubject};
use agentos_tools::ProcessRunner;
use serde::Deserialize;
use serde_json::Value;

use crate::{CaseResult, EvalError, EvalReport};

/// Runs every case in `cases_dir`.
pub fn run(cases_dir: &Path, fixtures_dir: &Path) -> Result<EvalReport, EvalError> {
    let fs = RealFs;
    let files = case_files(&fs, cases_dir)?;
    let mut cases = Vec::new();
    for path in files {
        let text = fs.read_to_string(&path)?;
        let value: Value = serde_json::from_str(&text).map_err(|err| EvalError::InvalidCase {
            path: path.clone(),
            message: err.to_string(),
        })?;
        let stem = path
            .file_stem()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let id = value.get("id").and_then(|id| id.as_str()).unwrap_or("");
        if id != stem {
            return Err(EvalError::InvalidCase {
                path,
                message: format!("id {id:?} must equal the file name {stem:?}"),
            });
        }
        let kind = value
            .get("kind")
            .and_then(|kind| kind.as_str())
            .unwrap_or("");
        let result = if kind == "regression" {
            let case: agentos_feedback::RegressionCase =
                serde_json::from_value(value).map_err(|err| EvalError::InvalidCase {
                    path: path.clone(),
                    message: err.to_string(),
                })?;
            execute_regression(&case)
        } else {
            let case: Case =
                serde_json::from_value(value).map_err(|err| EvalError::InvalidCase {
                    path: path.clone(),
                    message: err.to_string(),
                })?;
            if case.description.trim().is_empty() || case.oracle.trim().is_empty() {
                return Err(EvalError::InvalidCase {
                    path,
                    message: "oracle and description must not be empty".to_owned(),
                });
            }
            execute(&case, cases_dir, fixtures_dir)
        };
        cases.push(result);
    }
    Ok(EvalReport {
        area: "feedback".to_owned(),
        cases_dir: cases_dir.to_path_buf(),
        cases,
    })
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    id: String,
    kind: String,
    description: String,
    fixture: String,
    oracle: String,
    #[serde(default)]
    expected_providers: Vec<String>,
    #[serde(default)]
    config_toml: Option<String>,
    #[serde(default)]
    expected_error: Option<String>,
    #[serde(default)]
    provider: Option<String>,
    #[serde(default)]
    validation_kind: Option<String>,
    #[serde(default)]
    exit_code: Option<i32>,
    #[serde(default)]
    timed_out: bool,
    #[serde(default)]
    crashed: bool,
    #[serde(default)]
    spawn_error: Option<String>,
    #[serde(default)]
    expected_outcome: Option<String>,
    #[serde(default)]
    expected_file: Option<String>,
    #[serde(default)]
    expected_line: Option<u32>,
    #[serde(default)]
    recorded: Option<String>,
    #[serde(default)]
    expected_command: Vec<String>,
}

fn execute(case: &Case, cases_dir: &Path, fixtures_dir: &Path) -> CaseResult {
    let result = match case.kind.as_str() {
        "detect" => execute_detect(case, fixtures_dir),
        "parse" => execute_parse(case, cases_dir),
        "permission" => execute_permission(case, fixtures_dir),
        "command" => execute_command(case, fixtures_dir),
        other => Err(format!("unknown feedback case kind {other}")),
    };
    match result {
        Ok(detail) => passed(case, detail),
        Err(detail) => failed(case, detail),
    }
}

fn execute_detect(case: &Case, fixtures_dir: &Path) -> Result<String, String> {
    let copied = TempCopy::of(&fixtures_dir.join(&case.fixture))?;
    if let Some(text) = &case.config_toml {
        let dir = copied.path().join(".agentos");
        fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
        fs::write(dir.join("config.toml"), text).map_err(|err| err.to_string())?;
    }
    let config = agentos_feedback::load_project_config(&copied.path().join(".agentos/config.toml"))
        .map_err(|err| err.to_string())?;
    match agentos_feedback::detect(copied.path(), &config) {
        Err(err) => {
            let expected = case.expected_error.as_deref().unwrap_or("");
            if expected.is_empty() || !err.to_string().contains(expected) {
                Err(format!("error {err}; expected {expected:?}"))
            } else {
                Ok(format!("error contained {expected:?}"))
            }
        }
        Ok(providers) => {
            if case.expected_error.is_some() {
                return Err(format!("expected an error, detected {providers:?}"));
            }
            let ids: Vec<String> = providers.into_iter().map(|provider| provider.id).collect();
            if ids != case.expected_providers {
                Err(format!(
                    "providers {ids:?} != {:?}",
                    case.expected_providers
                ))
            } else {
                Ok(format!(
                    "providers {}/{}",
                    ids.len(),
                    case.expected_providers.len()
                ))
            }
        }
    }
}

fn execute_parse(case: &Case, cases_dir: &Path) -> Result<String, String> {
    let provider = case
        .provider
        .as_deref()
        .ok_or("parse case needs provider")?;
    let kind = parse_kind(case.validation_kind.as_deref().unwrap_or("unit_test"))?;
    let recorded = case
        .recorded
        .as_deref()
        .ok_or("parse case needs recorded")?;
    let stdout = fs::read_to_string(cases_dir.join(recorded)).map_err(|err| err.to_string())?;
    let captured = agentos_feedback::CapturedOutput {
        target: case.fixture.clone(),
        command: Vec::new(),
        exit_code: case.exit_code,
        stdout,
        stderr: String::new(),
        duration_ms: 1,
        timed_out: case.timed_out,
        crashed: case.crashed,
        spawn_error: case.spawn_error.clone(),
        truncated: false,
        task_id: None,
        context_pack_id: None,
    };
    let result = dispatch(provider, kind, &captured)?;
    let expected = case.expected_outcome.as_deref().unwrap_or("failed");
    if result.outcome.as_str() != expected {
        return Err(format!("outcome {} != {expected}", result.outcome.as_str()));
    }
    if let Some(file) = &case.expected_file {
        let found = result.diagnostics.iter().any(|diagnostic| {
            diagnostic.file.as_deref() == Some(file.as_str())
                && diagnostic.line == case.expected_line
        });
        if !found {
            return Err(format!(
                "missing {file}:{:?} in {:?}",
                case.expected_line, result.diagnostics
            ));
        }
    }
    Ok(format!(
        "{expected} diagnostics {}",
        result.diagnostics.len()
    ))
}

fn execute_command(case: &Case, fixtures_dir: &Path) -> Result<String, String> {
    let provider = case
        .provider
        .as_deref()
        .ok_or("command case needs provider")?;
    let kind = parse_kind(case.validation_kind.as_deref().unwrap_or("unit_test"))?;
    let root = fixtures_dir.join(&case.fixture);
    let command = match provider {
        "cargo" => CargoTestProvider::command(kind, None),
        "pytest" => PytestProvider::command(&root, kind, None),
        "go" => GoTestProvider::command(kind, None),
        "maven" => MavenProvider::command(kind, None),
        "gradle" => GradleProvider::command(&root, kind, None),
        "jest" => JestProvider::command(&root, None),
        "rspec" => RSpecProvider::command(&root, None),
        other => return Err(format!("unknown provider {other}")),
    }
    .map_err(|err| err.to_string())?;
    if command != case.expected_command {
        Err(format!(
            "command {command:?} != {:?}",
            case.expected_command
        ))
    } else {
        Ok(command.join(" "))
    }
}

fn execute_permission(case: &Case, fixtures_dir: &Path) -> Result<String, String> {
    let copied = TempCopy::of(&fixtures_dir.join(&case.fixture))?;
    let marker = copied.path().join("marker");
    let subject = Silent {
        permissions: PermissionSet::none(),
    };
    let request = agentos_feedback::RunRequest {
        root: copied.path(),
        kind: ValidationKind::UnitTest,
        tools: &ProcessRunner,
        subject: &subject,
        task_id: None,
        context_pack_id: None,
        command_override: Some(format!("/usr/bin/touch {}", marker.display())),
        timeout: Duration::from_secs(2),
        max_output_bytes: 1024,
    };
    let result = CargoTestProvider
        .run(&request)
        .map_err(|err| err.to_string())?;
    if result.outcome.as_str() != "refused" || result.passed || marker.exists() {
        return Err(format!(
            "outcome {} passed {} marker {}",
            result.outcome.as_str(),
            result.passed,
            marker.exists()
        ));
    }
    Ok("refused".to_owned())
}

fn execute_regression(case: &agentos_feedback::RegressionCase) -> CaseResult {
    match check_regression_case(case) {
        Ok(()) => CaseResult {
            id: case.id.clone(),
            kind: case.kind.clone(),
            description: case.description.clone(),
            passed: true,
            detail: format!("outcome {}", case.expected_outcome),
        },
        Err(detail) => CaseResult {
            id: case.id.clone(),
            kind: case.kind.clone(),
            description: case.description.clone(),
            passed: false,
            detail,
        },
    }
}

fn dispatch(
    provider: &str,
    kind: ValidationKind,
    captured: &agentos_feedback::CapturedOutput,
) -> Result<agentos_feedback::ValidationResult, String> {
    Ok(match provider {
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

fn parse_kind(value: &str) -> Result<ValidationKind, String> {
    value.parse().map_err(|err: String| err)
}

fn passed(case: &Case, detail: String) -> CaseResult {
    CaseResult {
        id: case.id.clone(),
        kind: case.kind.clone(),
        description: case.description.clone(),
        passed: true,
        detail,
    }
}

fn failed(case: &Case, detail: String) -> CaseResult {
    CaseResult {
        id: case.id.clone(),
        kind: case.kind.clone(),
        description: case.description.clone(),
        passed: false,
        detail,
    }
}

struct Silent {
    permissions: PermissionSet,
}

impl PermissionSubject for Silent {
    fn subject(&self) -> String {
        "skill 'silent'".to_owned()
    }

    fn permissions(&self) -> &PermissionSet {
        &self.permissions
    }
}

fn case_files(fs: &RealFs, cases_dir: &Path) -> Result<Vec<PathBuf>, EvalError> {
    if !fs.is_dir(cases_dir) {
        return Err(EvalError::NoCases(cases_dir.to_path_buf()));
    }
    let files: Vec<PathBuf> = fs
        .read_dir(cases_dir)?
        .into_iter()
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    if files.is_empty() {
        return Err(EvalError::NoCases(cases_dir.to_path_buf()));
    }
    Ok(files)
}

struct TempCopy {
    path: PathBuf,
}

impl TempCopy {
    fn of(source: &Path) -> Result<Self, String> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "agentos-feedback-eval-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        if path.exists() {
            fs::remove_dir_all(&path).map_err(|err| err.to_string())?;
        }
        copy_tree(source, &path)
            .map_err(|err| format!("cannot copy {}: {err}", source.display()))?;
        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempCopy {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    fs::create_dir_all(to)?;
    let mut entries: Vec<_> = fs::read_dir(from)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let name = entry.file_name();
        if name == ".agentos"
            || name == "target"
            || name == "__pycache__"
            || name == ".venv"
            || name == "node_modules"
        {
            continue;
        }
        let target = to.join(&name);
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else if file_type.is_file() {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}
