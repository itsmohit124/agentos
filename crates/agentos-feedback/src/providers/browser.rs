//! Browser validation provider.
//!
//! The default provider uses [`NullBrowser`]. [`BrowserProvider::fake`] uses [`FakeBrowser`]
//! for tests and evaluations. Both go through [`BrowserTool`], so a subject without `browser`
//! is refused before any page action. Detection is the presence of `expected/ui.json`. Projects
//! without that file are not browser projects.

use std::fs;
use std::path::Path;

use agentos_tools::{
    BROWSER_RUN_ID, BrowserCapability, BrowserStatus, BrowserStep, BrowserTool, FakeBrowser,
    Inspection, NullBrowser, prepare_run_artifacts,
};
use serde::Deserialize;

use crate::error::FeedbackError;
use crate::kind::ValidationKind;
use crate::provider::{RunRequest, ValidationProvider};
use crate::result::{
    Diagnostic, FeedbackItem, Outcome, ResultProvenance, ValidationResult, refused,
    sort_diagnostics,
};

/// Runs the browser assertions in `expected/ui.json`.
#[derive(Debug, Clone, Copy)]
pub struct BrowserProvider {
    fake: bool,
}

impl Default for BrowserProvider {
    fn default() -> Self {
        Self::null()
    }
}

impl BrowserProvider {
    /// The production provider. Actions report unavailable.
    pub fn null() -> Self {
        Self { fake: false }
    }

    /// Scripted provider for tests and evaluations.
    pub fn fake() -> Self {
        Self { fake: true }
    }

    /// True when `expected/ui.json` exists.
    pub fn applies(root: &Path) -> bool {
        root.join("expected/ui.json").is_file()
    }
}

impl ValidationProvider for BrowserProvider {
    fn id(&self) -> &'static str {
        "browser"
    }

    fn capabilities(&self) -> &'static [ValidationKind] {
        &[ValidationKind::Browser]
    }

    fn detect(&self, root: &Path) -> bool {
        Self::applies(root)
    }

    fn run(&self, request: &RunRequest<'_>) -> Result<ValidationResult, FeedbackError> {
        if self.fake {
            self.execute(request, FakeBrowser::new())
        } else {
            self.execute(request, NullBrowser)
        }
    }
}

impl BrowserProvider {
    fn execute<B: BrowserCapability>(
        &self,
        request: &RunRequest<'_>,
        browser: B,
    ) -> Result<ValidationResult, FeedbackError> {
        if request.kind != ValidationKind::Browser {
            return Err(FeedbackError::UnsupportedKind {
                provider: "browser".to_owned(),
                kind: request.kind,
            });
        }
        let ui = load_ui(request.root)?;
        let command = ui.command();
        let target = request.root.display().to_string();
        let mut tool = BrowserTool::new(browser);
        let mut inspection = None;
        for step in &ui.steps {
            let outcome = dispatch(&mut tool, request, step)?;
            match outcome {
                Drive::Denied(reason) => {
                    let mut result = refused(
                        "browser",
                        request.kind,
                        &target,
                        command,
                        &reason,
                        request.task_id.clone(),
                        request.context_pack_id.clone(),
                    );
                    result.capability = "browser".to_owned();
                    return Ok(result);
                }
                Drive::Stop(step) => {
                    let outcome = match step.status {
                        BrowserStatus::Unavailable => Outcome::Unavailable,
                        BrowserStatus::Failed => Outcome::Failed,
                        BrowserStatus::Completed => Outcome::Failed,
                    };
                    return Ok(finish(
                        request,
                        command,
                        outcome,
                        vec![step.detail],
                        Vec::new(),
                        if step.status == BrowserStatus::Unavailable {
                            "null-browser"
                        } else {
                            "browser-capability"
                        },
                    ));
                }
                Drive::Continue(step) => {
                    if let Some(found) = step.inspection {
                        inspection = Some(found);
                    }
                }
            }
        }
        let Some(inspection) = inspection else {
            return Ok(finish(
                request,
                command,
                Outcome::Failed,
                vec!["browser scenario did not inspect the page".to_owned()],
                Vec::new(),
                "browser-capability",
            ));
        };
        let problems = differences(&inspection, &ui.expect);
        if !problems.is_empty() {
            return Ok(finish(
                request,
                command,
                Outcome::Failed,
                problems,
                Vec::new(),
                "fake-browser",
            ));
        }
        let artifact = write_inspection(request.root, &inspection)?;
        Ok(finish(
            request,
            command,
            Outcome::Passed,
            Vec::new(),
            vec![artifact],
            "fake-browser",
        ))
    }
}

enum Drive {
    Denied(String),
    Stop(BrowserStep),
    Continue(BrowserStep),
}

fn dispatch<B: BrowserCapability>(
    tool: &mut BrowserTool<B>,
    request: &RunRequest<'_>,
    step: &UiStep,
) -> Result<Drive, FeedbackError> {
    let outcome = match step.action.as_str() {
        "open" => tool.open(request.subject, step.url.as_deref().unwrap_or("")),
        "fill" => tool.fill(
            request.subject,
            step.selector.as_deref().unwrap_or(""),
            step.value.as_deref().unwrap_or(""),
        ),
        "click" => tool.click(request.subject, step.selector.as_deref().unwrap_or("")),
        "inspect" => tool.inspect(request.subject),
        "screenshot" => tool.screenshot(request.subject, request.root, BROWSER_RUN_ID, "page.bin"),
        other => {
            return Ok(Drive::Stop(BrowserStep {
                action: other.to_owned(),
                status: BrowserStatus::Failed,
                detail: format!("unknown browser action {other}"),
                inspection: None,
                artifact: None,
            }));
        }
    };
    match outcome {
        Err(agentos_tools::ToolError::Denied { reason, .. }) => Ok(Drive::Denied(reason)),
        Err(other) => Err(other.into()),
        Ok(step) if step.status == BrowserStatus::Completed => Ok(Drive::Continue(step)),
        Ok(step) => Ok(Drive::Stop(step)),
    }
}

fn finish(
    request: &RunRequest<'_>,
    command: Vec<String>,
    outcome: Outcome,
    summaries: Vec<String>,
    artifacts: Vec<String>,
    method: &str,
) -> ValidationResult {
    let mut diagnostics: Vec<Diagnostic> = summaries
        .into_iter()
        .map(|summary| Diagnostic {
            tool: "browser".to_owned(),
            summary,
            file: None,
            line: None,
        })
        .collect();
    sort_diagnostics(&mut diagnostics);
    let provenance = ResultProvenance {
        kind: "observed".to_owned(),
        source: "browser".to_owned(),
        location: String::new(),
        extractor: "browser".to_owned(),
        extractor_version: "1".to_owned(),
        method: method.to_owned(),
    };
    let feedback = if outcome.is_pass() {
        Vec::new()
    } else {
        diagnostics
            .iter()
            .enumerate()
            .map(|(index, diagnostic)| FeedbackItem {
                id: format!("pending#{index}"),
                validation_id: String::new(),
                provider: "browser".to_owned(),
                kind: request.kind,
                summary: diagnostic.summary.clone(),
                file: None,
                line: None,
                task_id: request.task_id.clone(),
                context_pack_id: request.context_pack_id.clone(),
                provenance: provenance.clone(),
            })
            .collect()
    };
    ValidationResult {
        id: String::new(),
        provider: "browser".to_owned(),
        kind: request.kind,
        target: request.root.display().to_string(),
        command,
        capability: "browser".to_owned(),
        exit_status: match outcome {
            Outcome::Passed => Some(0),
            Outcome::Failed => Some(1),
            Outcome::Refused | Outcome::Unavailable | Outcome::TimedOut | Outcome::Crashed => None,
        },
        passed: outcome.is_pass(),
        outcome,
        duration_ms: 0,
        diagnostics,
        raw_output_ref: None,
        artifacts,
        provenance,
        task_id: request.task_id.clone(),
        context_pack_id: request.context_pack_id.clone(),
        feedback,
        output_truncated: false,
    }
}

fn write_inspection(root: &Path, inspection: &Inspection) -> Result<String, FeedbackError> {
    let dir = prepare_run_artifacts(root, BROWSER_RUN_ID)?;
    let path = dir.join("inspect.json");
    let body = serde_json::to_string_pretty(&InspectFile {
        console: &inspection.console,
        dom: &inspection.dom,
        network: inspection
            .network
            .iter()
            .map(|item| InspectNetwork {
                method: &item.method,
                url: &item.url,
            })
            .collect::<Vec<_>>(),
    })
    .map_err(|err| FeedbackError::Config(err.to_string()))?;
    fs::write(&path, format!("{body}\n"))?;
    let root = root.canonicalize()?;
    let file = path.canonicalize()?;
    if !file.starts_with(&root) {
        let _ = fs::remove_file(&file);
        return Err(FeedbackError::Config(
            "browser artifact escaped the project root".to_owned(),
        ));
    }
    Ok(format!(
        ".agentos/runs/{BROWSER_RUN_ID}/artifacts/inspect.json"
    ))
}

fn differences(inspection: &Inspection, expect: &UiExpect) -> Vec<String> {
    let mut problems = Vec::new();
    if inspection.dom != expect.dom {
        problems.push(format!("dom {:?} != {:?}", inspection.dom, expect.dom));
    }
    if inspection.console != expect.console {
        problems.push(format!(
            "console {:?} != {:?}",
            inspection.console, expect.console
        ));
    }
    let actual: Vec<(&str, &str)> = inspection
        .network
        .iter()
        .map(|item| (item.method.as_str(), item.url.as_str()))
        .collect();
    let expected: Vec<(&str, &str)> = expect
        .network
        .iter()
        .map(|item| (item.method.as_str(), item.url.as_str()))
        .collect();
    if actual != expected {
        problems.push(format!("network {actual:?} != {expected:?}"));
    }
    problems
}

fn load_ui(root: &Path) -> Result<UiFile, FeedbackError> {
    let path = root.join("expected/ui.json");
    let text = fs::read_to_string(&path)
        .map_err(|err| FeedbackError::Config(format!("cannot read {}: {err}", path.display())))?;
    serde_json::from_str(&text)
        .map_err(|err| FeedbackError::Config(format!("invalid {}: {err}", path.display())))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct UiFile {
    steps: Vec<UiStep>,
    expect: UiExpect,
}

impl UiFile {
    fn command(&self) -> Vec<String> {
        self.steps
            .iter()
            .map(|step| match step.action.as_str() {
                "open" => format!("open {}", step.url.as_deref().unwrap_or("")),
                "fill" => format!(
                    "fill {} {}",
                    step.selector.as_deref().unwrap_or(""),
                    step.value.as_deref().unwrap_or("")
                ),
                "click" => format!("click {}", step.selector.as_deref().unwrap_or("")),
                other => other.to_owned(),
            })
            .collect()
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct UiStep {
    action: String,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    selector: Option<String>,
    #[serde(default)]
    value: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct UiExpect {
    console: Vec<String>,
    dom: Vec<String>,
    network: Vec<UiNetwork>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct UiNetwork {
    method: String,
    url: String,
}

#[derive(serde::Serialize)]
struct InspectFile<'a> {
    console: &'a [String],
    dom: &'a [String],
    network: Vec<InspectNetwork<'a>>,
}

#[derive(serde::Serialize)]
struct InspectNetwork<'a> {
    method: &'a str,
    url: &'a str,
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use agentos_permissions::{Capability, PermissionSet, PermissionSubject};
    use agentos_tools::{ExecRequest, ProcessRunner, ToolError, ToolRunner};
    use tempfile::tempdir;

    use super::*;
    use crate::kind::ValidationKind;

    struct Skill {
        name: &'static str,
        permissions: PermissionSet,
    }

    impl PermissionSubject for Skill {
        fn subject(&self) -> String {
            format!("skill '{}'", self.name)
        }
        fn permissions(&self) -> &PermissionSet {
            &self.permissions
        }
    }

    struct PanicRunner;

    impl ToolRunner for PanicRunner {
        fn execute(
            &self,
            _subject: &dyn PermissionSubject,
            _request: &ExecRequest,
        ) -> Result<agentos_tools::ExecOutcome, ToolError> {
            panic!("browser validation must not spawn a process");
        }
    }

    const UI: &str = r##"{
  "steps": [
    {"action": "open", "url": "http://payments.local/checkout"},
    {"action": "fill", "selector": "#amount", "value": "42"},
    {"action": "click", "selector": "#charge"},
    {"action": "inspect"}
  ],
  "expect": {
    "console": ["charge submitted"],
    "dom": ["button#charge=clicked", "input#amount=42"],
    "network": [{"method": "POST", "url": "http://payments.local/charge"}]
  }
}
"##;

    fn project() -> tempfile::TempDir {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("expected")).unwrap();
        fs::write(dir.path().join("expected/ui.json"), UI).unwrap();
        dir
    }

    fn request<'a>(
        root: &'a Path,
        subject: &'a dyn PermissionSubject,
        tools: &'a dyn ToolRunner,
    ) -> RunRequest<'a> {
        RunRequest {
            root,
            kind: ValidationKind::Browser,
            tools,
            subject,
            task_id: None,
            context_pack_id: None,
            command_override: None,
            timeout: Duration::from_secs(1),
            max_output_bytes: 1024,
        }
    }

    #[test]
    fn a_skill_without_browser_is_refused() {
        let dir = project();
        let subject = Skill {
            name: "ui-review",
            permissions: [Capability::Shell, Capability::FsRead]
                .into_iter()
                .collect(),
        };
        let result = BrowserProvider::fake()
            .run(&request(dir.path(), &subject, &PanicRunner))
            .unwrap();
        assert_eq!(result.outcome, Outcome::Refused);
        assert!(!result.passed);
        assert_eq!(result.capability, "browser");
        assert!(
            result.diagnostics[0]
                .summary
                .contains("does not declare browser")
        );
        assert!(!dir.path().join(".agentos/runs").exists());
    }

    #[test]
    fn null_browser_is_unavailable_and_writes_nothing() {
        let dir = project();
        let subject = Skill {
            name: "ui-review",
            permissions: [Capability::Browser].into_iter().collect(),
        };
        let result = BrowserProvider::null()
            .run(&request(dir.path(), &subject, &ProcessRunner))
            .unwrap();
        assert_eq!(result.outcome, Outcome::Unavailable);
        assert!(!result.passed);
        assert_eq!(result.capability, "browser");
        assert!(result.artifacts.is_empty());
        assert_eq!(
            result.diagnostics[0].summary,
            "browser unavailable; no page was opened or changed"
        );
        assert!(!result.diagnostics[0].summary.contains("payments.local"));
        assert!(!dir.path().join(".agentos/runs").exists());
    }

    #[test]
    fn fake_browser_matches_the_checkout_scenario() {
        let dir = project();
        let subject = Skill {
            name: "ui-review",
            permissions: [Capability::Browser].into_iter().collect(),
        };
        let result = BrowserProvider::fake()
            .run(&request(dir.path(), &subject, &PanicRunner))
            .unwrap();
        assert_eq!(result.outcome, Outcome::Passed, "{:?}", result.diagnostics);
        assert!(result.passed);
        assert_eq!(
            result.artifacts,
            vec![".agentos/runs/browser-ui/artifacts/inspect.json".to_owned()]
        );
        let path = dir
            .path()
            .join(".agentos/runs/browser-ui/artifacts/inspect.json");
        let canon = path.canonicalize().unwrap();
        let artifacts = dir
            .path()
            .join(".agentos/runs/browser-ui/artifacts")
            .canonicalize()
            .unwrap();
        assert!(canon.starts_with(&artifacts));
        let body: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            body["dom"],
            serde_json::json!(["button#charge=clicked", "input#amount=42"])
        );
        assert_eq!(body["console"], serde_json::json!(["charge submitted"]));
        assert_eq!(
            body["network"],
            serde_json::json!([{"method": "POST", "url": "http://payments.local/charge"}])
        );
    }

    #[test]
    fn detection_requires_ui_assertions() {
        let empty = tempdir().unwrap();
        assert!(!BrowserProvider::null().detect(empty.path()));
        let dir = project();
        assert!(BrowserProvider::null().detect(dir.path()));
        assert_eq!(
            BrowserProvider::null().capabilities(),
            &[ValidationKind::Browser]
        );
    }
}
