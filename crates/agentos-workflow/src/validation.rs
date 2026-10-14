//! Phase 5 validation as a workflow input.
//!
//! The engine decides from [`ValidationResult`] and [`FeedbackItem`]. It does not
//! parse tool output. [`ScriptedValidator`] feeds those records in tests.
//! [`Phase5Validator`] asks the feedback engine and passes the records through.

use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;

use agentos_feedback::{
    ArchitectureReport, Diagnostic, FeedbackItem, Outcome, ResultProvenance, ValidationKind,
    ValidationResult, analyze_project, load_project_config, run_and_store,
};
use agentos_permissions::{Capability, PermissionSet, PermissionSubject};
use agentos_tools::ProcessRunner;
use serde::{Deserialize, Serialize};

use crate::error::WorkflowError;

/// What a step asks the feedback engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationRequest {
    /// Step id.
    pub step_id: String,
    /// 1-based attempt.
    pub attempt: u32,
    /// Validation kind wire name.
    pub kind: String,
    /// User task.
    pub task_id: String,
    /// Context Pack id.
    pub context_pack_id: String,
    /// Capabilities the step declared.
    pub granted: Vec<String>,
    /// Project root.
    pub target: String,
}

/// Produces a [`ValidationResult`] for one attempt.
pub trait Validator {
    /// Runs the requested validation.
    fn validate(&mut self, request: &ValidationRequest) -> Result<ValidationResult, WorkflowError>;
}

/// True when the result is not a pass. The decision reads the record, not a log.
pub fn rejected(result: &ValidationResult) -> bool {
    !result.passed || !result.outcome.is_pass()
}

/// Feedback summaries, in record order.
pub fn feedback_text(result: &ValidationResult) -> String {
    if result.feedback.is_empty() {
        result.outcome.as_str().to_owned()
    } else {
        result
            .feedback
            .iter()
            .map(|item| item.summary.clone())
            .collect::<Vec<_>>()
            .join("; ")
    }
}

/// A deterministic validator. `false` is a failed [`ValidationResult`] with one feedback item.
#[derive(Debug)]
pub struct ScriptedValidator {
    scripts: BTreeMap<String, VecDeque<bool>>,
    default_pass: bool,
    /// Step ids in call order.
    pub calls: Vec<String>,
}

impl ScriptedValidator {
    /// Every validation passes.
    pub fn pass() -> Self {
        Self {
            scripts: BTreeMap::new(),
            default_pass: true,
            calls: Vec::new(),
        }
    }

    /// Queues pass (`true`) or fail (`false`) for each attempt of `step_id`.
    pub fn script(mut self, step_id: &str, outcomes: &[bool]) -> Self {
        self.scripts
            .insert(step_id.to_owned(), VecDeque::from(outcomes.to_vec()));
        self
    }
}

impl Validator for ScriptedValidator {
    fn validate(&mut self, request: &ValidationRequest) -> Result<ValidationResult, WorkflowError> {
        self.calls.push(request.step_id.clone());
        let pass = self
            .scripts
            .get_mut(&request.step_id)
            .and_then(VecDeque::pop_front)
            .unwrap_or(self.default_pass);
        Ok(scripted_result(request, pass))
    }
}

/// Calls Phase 5 and returns its [`ValidationResult`].
pub struct Phase5Validator {
    /// Project root.
    pub root: PathBuf,
}

impl Phase5Validator {
    /// Validator for `root`.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }
}

impl Validator for Phase5Validator {
    fn validate(&mut self, request: &ValidationRequest) -> Result<ValidationResult, WorkflowError> {
        let kind: ValidationKind = request
            .kind
            .parse()
            .map_err(|err: String| WorkflowError::Validation(err))?;
        if kind == ValidationKind::Architecture {
            let config = load_project_config(&self.root.join(".agentos/config.toml"))
                .map_err(|err| WorkflowError::Validation(err.to_string()))?;
            let report = analyze_project(&self.root, &config.architecture)
                .map_err(|err| WorkflowError::Validation(err.to_string()))?;
            return Ok(from_architecture(&report, request));
        }
        let subject = Granted::new(&request.step_id, &request.granted)?;
        let stored = run_and_store(
            &self.root,
            kind,
            &ProcessRunner,
            &subject,
            Some(request.task_id.clone()),
            Some(request.context_pack_id.clone()),
        )
        .map_err(|err| WorkflowError::Validation(err.to_string()))?;
        Ok(fold_results(stored, request))
    }
}

/// Builds a failed or passed record without reading tool text.
pub fn scripted_result(request: &ValidationRequest, pass: bool) -> ValidationResult {
    let kind = request.kind.parse().unwrap_or(ValidationKind::UnitTest);
    let id = format!("{}-attempt-{}", request.step_id, request.attempt);
    let provenance = provenance_for("workflow-script");
    let (outcome, diagnostics, feedback) = if pass {
        (Outcome::Passed, Vec::new(), Vec::new())
    } else {
        let diagnostic = Diagnostic {
            tool: "workflow".to_owned(),
            summary: "validation failed".to_owned(),
            file: None,
            line: None,
        };
        let item = FeedbackItem {
            id: format!("{id}#0"),
            validation_id: id.clone(),
            provider: "workflow".to_owned(),
            kind,
            summary: diagnostic.summary.clone(),
            file: None,
            line: None,
            task_id: Some(request.task_id.clone()),
            context_pack_id: Some(request.context_pack_id.clone()),
            provenance: provenance.clone(),
        };
        (Outcome::Failed, vec![diagnostic], vec![item])
    };
    ValidationResult {
        id,
        provider: "workflow".to_owned(),
        kind,
        target: request.target.clone(),
        command: Vec::new(),
        capability: if kind == ValidationKind::Browser {
            "browser".to_owned()
        } else {
            "shell".to_owned()
        },
        exit_status: Some(if pass { 0 } else { 1 }),
        passed: pass,
        outcome,
        duration_ms: 0,
        diagnostics,
        raw_output_ref: None,
        artifacts: Vec::new(),
        provenance,
        task_id: Some(request.task_id.clone()),
        context_pack_id: Some(request.context_pack_id.clone()),
        feedback,
        output_truncated: false,
    }
}

/// Turns architecture findings into the validation record the workflow already consumes.
pub fn from_architecture(
    report: &ArchitectureReport,
    request: &ValidationRequest,
) -> ValidationResult {
    let id = format!("{}-attempt-{}", request.step_id, request.attempt);
    let provenance = provenance_for("architecture");
    let mut feedback = Vec::new();
    let mut diagnostics = Vec::new();
    for (index, finding) in report.findings.iter().enumerate() {
        let file = finding.paths.first().cloned();
        diagnostics.push(Diagnostic {
            tool: finding.detector.clone(),
            summary: finding.summary.clone(),
            file: file.clone(),
            line: None,
        });
        feedback.push(FeedbackItem {
            id: format!("{id}#{index}"),
            validation_id: id.clone(),
            provider: "architecture".to_owned(),
            kind: ValidationKind::Architecture,
            summary: finding.summary.clone(),
            file,
            line: None,
            task_id: Some(request.task_id.clone()),
            context_pack_id: Some(request.context_pack_id.clone()),
            provenance: finding.provenance.clone(),
        });
    }
    let passed = feedback.is_empty();
    ValidationResult {
        id,
        provider: "architecture".to_owned(),
        kind: ValidationKind::Architecture,
        target: request.target.clone(),
        command: Vec::new(),
        capability: "fs.read".to_owned(),
        exit_status: None,
        passed,
        outcome: if passed {
            Outcome::Passed
        } else {
            Outcome::Failed
        },
        duration_ms: 0,
        diagnostics,
        raw_output_ref: None,
        artifacts: Vec::new(),
        provenance,
        task_id: Some(request.task_id.clone()),
        context_pack_id: Some(request.context_pack_id.clone()),
        feedback,
        output_truncated: false,
    }
}

fn fold_results(
    mut results: Vec<ValidationResult>,
    request: &ValidationRequest,
) -> ValidationResult {
    if results.is_empty() {
        return unavailable(request);
    }
    if let Some(index) = results.iter().position(rejected) {
        return results.swap_remove(index);
    }
    results.remove(0)
}

fn unavailable(request: &ValidationRequest) -> ValidationResult {
    let mut result = scripted_result(request, false);
    result.provider = "workflow".to_owned();
    result.outcome = Outcome::Unavailable;
    result.passed = false;
    result.exit_status = None;
    result.diagnostics.clear();
    result.feedback.clear();
    let provenance = provenance_for("workflow");
    let summary = format!("no provider supports {}", request.kind);
    result.diagnostics.push(Diagnostic {
        tool: "workflow".to_owned(),
        summary: summary.clone(),
        file: None,
        line: None,
    });
    result.feedback.push(FeedbackItem {
        id: format!("{}#0", result.id),
        validation_id: result.id.clone(),
        provider: "workflow".to_owned(),
        kind: request.kind.parse().unwrap_or(ValidationKind::UnitTest),
        summary,
        file: None,
        line: None,
        task_id: Some(request.task_id.clone()),
        context_pack_id: Some(request.context_pack_id.clone()),
        provenance: provenance.clone(),
    });
    result.provenance = provenance;
    result
}

fn provenance_for(method: &str) -> ResultProvenance {
    ResultProvenance {
        kind: "observed".to_owned(),
        source: "workflow".to_owned(),
        location: String::new(),
        extractor: "workflow".to_owned(),
        extractor_version: "1".to_owned(),
        method: method.to_owned(),
    }
}

struct Granted {
    label: String,
    permissions: PermissionSet,
}

impl Granted {
    fn new(step_id: &str, granted: &[String]) -> Result<Self, WorkflowError> {
        let mut caps = Vec::new();
        for name in granted {
            let cap: Capability =
                name.parse()
                    .map_err(|err: agentos_permissions::UnknownCapability| {
                        WorkflowError::Validation(err.to_string())
                    })?;
            caps.push(cap);
        }
        Ok(Self {
            label: format!("step '{step_id}'"),
            permissions: caps.into_iter().collect(),
        })
    }
}

impl PermissionSubject for Granted {
    fn subject(&self) -> String {
        self.label.clone()
    }

    fn permissions(&self) -> &PermissionSet {
        &self.permissions
    }
}

/// A recorded permission decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionCheck {
    /// Capability wire name.
    pub capability: String,
    /// `allow` or `deny`.
    pub verdict: String,
    /// Reason from the permission system.
    pub reason: String,
}
