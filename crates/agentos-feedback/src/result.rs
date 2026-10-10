//! `ValidationResult` and `FeedbackItem`.
//!
//! Every field is always serialized. A provider that does not know a value stores `null` or an
//! empty list. It does not omit the field. Raw process output is not copied into the result;
//! `raw_output_ref` points at a capped artifact.

use serde::{Deserialize, Serialize};

use crate::kind::ValidationKind;

/// How a run finished. `passed` on the result is true only for [`Outcome::Passed`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// The process exited 0. A source location in the output does not override that status.
    Passed,
    /// The process failed, or the parser found a failure.
    Failed,
    /// Shell permission was missing. Nothing was spawned.
    Refused,
    /// The timeout elapsed and the process was killed.
    TimedOut,
    /// The process died from a signal.
    Crashed,
    /// The program could not be started, or the runtime is missing.
    Unavailable,
}

impl Outcome {
    /// Wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Passed => "passed",
            Self::Failed => "failed",
            Self::Refused => "refused",
            Self::TimedOut => "timed_out",
            Self::Crashed => "crashed",
            Self::Unavailable => "unavailable",
        }
    }

    /// True only for a real pass. Refused, skipped, and crashed runs are not passes.
    pub fn is_pass(self) -> bool {
        self == Self::Passed
    }
}

/// Why a validation result is believed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResultProvenance {
    /// `observed` for tool output and permission checks. `inferred` for architecture heuristics.
    pub kind: String,
    /// Provider id, permission subject, or source path.
    pub source: String,
    /// `file:line` when a single location explains the result. Empty when it does not.
    pub location: String,
    /// Component that produced the record (`cargo`, `architecture`, …).
    pub extractor: String,
    /// Extractor version. `1` for this phase.
    pub extractor_version: String,
    /// How the record was obtained (`process-output`, `permission-check`, `import-edge`, …).
    pub method: String,
}

/// One structured diagnostic. `file` and `line` are null when the tool did not report them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    /// Provider or tool that emitted the diagnostic.
    pub tool: String,
    /// One-line summary. Never empty.
    pub summary: String,
    /// Source file, as reported by the tool. Null when unknown.
    pub file: Option<String>,
    /// 1-based line. Null when unknown.
    pub line: Option<u32>,
}

/// A failed validation linked to a task or Context Pack when the caller supplied one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeedbackItem {
    /// `{validation_id}#{index}` after the result is stored.
    pub id: String,
    /// Id of the [`ValidationResult`] this item came from.
    pub validation_id: String,
    /// Provider id.
    pub provider: String,
    /// Validation kind.
    pub kind: ValidationKind,
    /// Diagnostic summary.
    pub summary: String,
    /// Source file. Null when the diagnostic has none.
    pub file: Option<String>,
    /// Source line. Null when the diagnostic has none.
    pub line: Option<u32>,
    /// Caller-supplied task id. Null when the run was not tied to a task.
    pub task_id: Option<String>,
    /// Caller-supplied Context Pack id. Null when the run was not tied to a pack.
    pub context_pack_id: Option<String>,
    /// Same provenance as the validation result.
    pub provenance: ResultProvenance,
}

/// One validation run.
///
/// `id` is empty until the result is stored. `exit_status`, `raw_output_ref`, `task_id`, and
/// `context_pack_id` are null when that information does not exist.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationResult {
    /// Stable id assigned when the result is stored. Empty before that.
    pub id: String,
    /// Provider id (`cargo`, `pytest`, …).
    pub provider: String,
    /// Kind that was requested.
    pub kind: ValidationKind,
    /// Project root or other scope that was validated.
    pub target: String,
    /// Argv that would be executed. Empty when the provider could not build a command.
    pub command: Vec<String>,
    /// Capability required to run the command. `shell` for process providers.
    pub capability: String,
    /// Process exit code. Null when the process did not exit normally.
    pub exit_status: Option<i32>,
    /// True only when `outcome` is [`Outcome::Passed`].
    pub passed: bool,
    /// Pass, fail, refusal, timeout, crash, or an unavailable runtime.
    pub outcome: Outcome,
    /// Milliseconds spent waiting. `0` when no process was started.
    pub duration_ms: u64,
    /// Structured diagnostics, sorted by file, line, then summary.
    pub diagnostics: Vec<Diagnostic>,
    /// Path of the capped raw output, relative to the project root. Null when nothing was stored.
    pub raw_output_ref: Option<String>,
    /// Related artifact paths. Does not contain the process output itself.
    pub artifacts: Vec<String>,
    /// Why this result is believed.
    pub provenance: ResultProvenance,
    /// Task id supplied by the caller. Null when absent.
    pub task_id: Option<String>,
    /// Context Pack id supplied by the caller. Null when absent.
    pub context_pack_id: Option<String>,
    /// Feedback items. Empty when the run passed.
    pub feedback: Vec<FeedbackItem>,
    /// True when stdout or stderr hit the byte cap.
    pub output_truncated: bool,
}

impl ValidationResult {
    /// Writes the stored id onto the result and its feedback items.
    pub fn assign_id(&mut self, id: impl Into<String>) {
        let id = id.into();
        self.id = id.clone();
        for (index, item) in self.feedback.iter_mut().enumerate() {
            item.validation_id = id.clone();
            item.id = format!("{id}#{index}");
        }
    }
}

/// Process output handed to a parser. The parser does not spawn anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedOutput {
    /// Project root the command ran in.
    pub target: String,
    /// Argv.
    pub command: Vec<String>,
    /// Exit code. Null on timeout, crash, or spawn failure.
    pub exit_code: Option<i32>,
    /// Capped stdout.
    pub stdout: String,
    /// Capped stderr.
    pub stderr: String,
    /// Wait time in milliseconds.
    pub duration_ms: u64,
    /// Timeout killed the process.
    pub timed_out: bool,
    /// Signal death. Not set for timeouts.
    pub crashed: bool,
    /// Spawn error text. Null when the process started.
    pub spawn_error: Option<String>,
    /// Stdout or stderr was truncated.
    pub truncated: bool,
    /// Task id to copy onto the result.
    pub task_id: Option<String>,
    /// Context Pack id to copy onto the result.
    pub context_pack_id: Option<String>,
}

/// Builds a result from parser diagnostics. Outcome follows the process status, not the fact
/// that a process was launched.
pub fn assemble(
    provider: &str,
    kind: ValidationKind,
    captured: &CapturedOutput,
    diagnostics: Vec<Diagnostic>,
) -> ValidationResult {
    let mut diagnostics = diagnostics;
    let outcome = if captured.spawn_error.is_some() {
        Outcome::Unavailable
    } else if captured.timed_out {
        Outcome::TimedOut
    } else if captured.crashed {
        Outcome::Crashed
    } else if captured.exit_code == Some(0) {
        Outcome::Passed
    } else {
        Outcome::Failed
    };
    if diagnostics.is_empty()
        && let Some(summary) = fallback_summary(outcome, captured)
    {
        diagnostics.push(Diagnostic {
            tool: provider.to_owned(),
            summary,
            file: None,
            line: None,
        });
    }
    sort_diagnostics(&mut diagnostics);
    let provenance = ResultProvenance {
        kind: "observed".to_owned(),
        source: provider.to_owned(),
        location: String::new(),
        extractor: provider.to_owned(),
        extractor_version: "1".to_owned(),
        method: if captured.spawn_error.is_some() {
            "spawn".to_owned()
        } else {
            "process-output".to_owned()
        },
    };
    let feedback = feedback_for(provider, kind, &provenance, &diagnostics, captured, outcome);
    ValidationResult {
        id: String::new(),
        provider: provider.to_owned(),
        kind,
        target: captured.target.clone(),
        command: captured.command.clone(),
        capability: "shell".to_owned(),
        exit_status: captured.exit_code,
        passed: outcome.is_pass(),
        outcome,
        duration_ms: captured.duration_ms,
        diagnostics,
        raw_output_ref: None,
        artifacts: Vec::new(),
        provenance,
        task_id: captured.task_id.clone(),
        context_pack_id: captured.context_pack_id.clone(),
        feedback,
        output_truncated: captured.truncated,
    }
}

/// A refusal recorded before any process starts.
pub fn refused(
    provider: &str,
    kind: ValidationKind,
    target: &str,
    command: Vec<String>,
    reason: &str,
    task_id: Option<String>,
    context_pack_id: Option<String>,
) -> ValidationResult {
    let provenance = ResultProvenance {
        kind: "observed".to_owned(),
        source: "permission".to_owned(),
        location: String::new(),
        extractor: provider.to_owned(),
        extractor_version: "1".to_owned(),
        method: "permission-check".to_owned(),
    };
    let diagnostic = Diagnostic {
        tool: provider.to_owned(),
        summary: reason.to_owned(),
        file: None,
        line: None,
    };
    let captured = CapturedOutput {
        target: target.to_owned(),
        command: command.clone(),
        exit_code: None,
        stdout: String::new(),
        stderr: String::new(),
        duration_ms: 0,
        timed_out: false,
        crashed: false,
        spawn_error: None,
        truncated: false,
        task_id: task_id.clone(),
        context_pack_id: context_pack_id.clone(),
    };
    let feedback = feedback_for(
        provider,
        kind,
        &provenance,
        std::slice::from_ref(&diagnostic),
        &captured,
        Outcome::Refused,
    );
    ValidationResult {
        id: String::new(),
        provider: provider.to_owned(),
        kind,
        target: target.to_owned(),
        command,
        capability: "shell".to_owned(),
        exit_status: None,
        passed: false,
        outcome: Outcome::Refused,
        duration_ms: 0,
        diagnostics: vec![diagnostic],
        raw_output_ref: None,
        artifacts: Vec::new(),
        provenance,
        task_id,
        context_pack_id,
        feedback,
        output_truncated: false,
    }
}

fn feedback_for(
    provider: &str,
    kind: ValidationKind,
    provenance: &ResultProvenance,
    diagnostics: &[Diagnostic],
    captured: &CapturedOutput,
    outcome: Outcome,
) -> Vec<FeedbackItem> {
    if outcome.is_pass() {
        return Vec::new();
    }
    diagnostics
        .iter()
        .enumerate()
        .map(|(index, diagnostic)| FeedbackItem {
            id: format!("pending#{index}"),
            validation_id: String::new(),
            provider: provider.to_owned(),
            kind,
            summary: diagnostic.summary.clone(),
            file: diagnostic.file.clone(),
            line: diagnostic.line,
            task_id: captured.task_id.clone(),
            context_pack_id: captured.context_pack_id.clone(),
            provenance: provenance.clone(),
        })
        .collect()
}

fn fallback_summary(outcome: Outcome, captured: &CapturedOutput) -> Option<String> {
    match outcome {
        Outcome::Passed | Outcome::Refused => None,
        Outcome::Unavailable => Some(
            captured
                .spawn_error
                .clone()
                .unwrap_or_else(|| "command could not be started".to_owned()),
        ),
        Outcome::TimedOut => Some(format!("timed out after {}ms", captured.duration_ms)),
        Outcome::Crashed => Some("process crashed".to_owned()),
        Outcome::Failed => Some(match captured.exit_code {
            Some(code) => format!("process exited with status {code}"),
            None => "process failed".to_owned(),
        }),
    }
}

/// Sorts diagnostics so repeated parses emit the same JSON.
pub fn sort_diagnostics(diagnostics: &mut Vec<Diagnostic>) {
    diagnostics.sort_by(|left, right| {
        (
            left.file.as_deref().unwrap_or(""),
            left.line.unwrap_or(0),
            left.summary.as_str(),
        )
            .cmp(&(
                right.file.as_deref().unwrap_or(""),
                right.line.unwrap_or(0),
                right.summary.as_str(),
            ))
    });
    diagnostics.dedup();
}

/// One-line summary, capped so a raw dump cannot land in the record.
pub fn one_line(text: &str) -> String {
    let line = text
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or(text);
    let trimmed = line.trim();
    if trimmed.chars().count() <= 200 {
        trimmed.to_owned()
    } else {
        trimmed.chars().take(200).collect()
    }
}
