//! Recording after a workflow run, and the summarisation seam.
//!
//! [`StructuredSummarizer`] joins the fields it is given. It does not call a model.

use std::collections::BTreeMap;

use agentos_knowledge::ProvenanceKind;

use crate::error::MemoryError;
use crate::model::{MemoryLink, MemoryRecord, NewMemory};
use crate::store::MemoryStore;

/// Structured fields captured from a finished run.
#[derive(Debug, Clone)]
pub struct RunCapture {
    /// Run id.
    pub run_id: String,
    /// User task, unchanged except for redaction.
    pub task: String,
    /// Final outcome (`completed`, `failed`, `aborted`, `blocked`).
    pub outcome: String,
    /// One Context Pack id from the run.
    pub context_pack_id: String,
    /// Every Context Pack id, sorted by the caller.
    pub pack_ids: Vec<String>,
    /// Agent result summary. Redacted before it is stored.
    pub agent_summary: String,
    /// Validation results, already rendered as text.
    pub validation: String,
    /// Failure texts.
    pub failures: Vec<String>,
    /// Fingerprints of the code and domain the run used.
    pub links: Vec<MemoryLink>,
    /// Caller-supplied timestamp.
    pub recorded_at: String,
    /// Intent kind of the task. Used by a successful context pattern.
    pub intent_kind: String,
    /// Selected node types that worked, sorted.
    pub shape: Vec<String>,
}

/// Input to a summariser. Lifetimes stay with the caller.
pub struct SummaryInput<'a> {
    /// Task.
    pub task: &'a str,
    /// Run id.
    pub run_id: &'a str,
    /// Outcome.
    pub outcome: &'a str,
    /// Context Pack id.
    pub context_pack_id: &'a str,
    /// Validation text.
    pub validation: &'a str,
    /// Failures.
    pub failures: &'a [String],
    /// Agent summary.
    pub agent_summary: &'a str,
}

/// Future summarisation seam. Implementations must not call a model in this phase.
pub trait Summarizer {
    /// Returns a deterministic summary of `input`.
    fn summarize(&self, input: &SummaryInput<'_>) -> String;
}

/// Joins the structured fields. No model and no network.
#[derive(Debug, Default, Clone, Copy)]
pub struct StructuredSummarizer;

impl Summarizer for StructuredSummarizer {
    fn summarize(&self, input: &SummaryInput<'_>) -> String {
        let mut failures = input.failures.to_vec();
        failures.sort();
        format!(
            "task: {}\nrun: {}\noutcome: {}\ncontext_pack: {}\nvalidation: {}\nfailures: {}\nagent: {}\n",
            input.task,
            input.run_id,
            input.outcome,
            input.context_pack_id,
            input.validation,
            failures.join("; "),
            input.agent_summary
        )
    }
}

/// Records an episode and, when the run failed, a proposed lesson.
///
/// A completed run also records a procedural context pattern.
pub fn record_run(
    store: &MemoryStore,
    capture: &RunCapture,
) -> Result<Vec<MemoryRecord>, MemoryError> {
    record_run_with(store, capture, &StructuredSummarizer)
}

/// Same as [`record_run`] with a caller-supplied summariser.
pub fn record_run_with(
    store: &MemoryStore,
    capture: &RunCapture,
    summarizer: &dyn Summarizer,
) -> Result<Vec<MemoryRecord>, MemoryError> {
    let summary = summarizer.summarize(&SummaryInput {
        task: &capture.task,
        run_id: &capture.run_id,
        outcome: &capture.outcome,
        context_pack_id: &capture.context_pack_id,
        validation: &capture.validation,
        failures: &capture.failures,
        agent_summary: &capture.agent_summary,
    });
    let mut payload = BTreeMap::new();
    payload.insert(
        "context_pack_id".to_owned(),
        capture.context_pack_id.clone(),
    );
    let mut pack_ids = capture.pack_ids.clone();
    pack_ids.sort();
    payload.insert("pack_ids".to_owned(), pack_ids.join(","));
    payload.insert("validation".to_owned(), capture.validation.clone());
    let mut failures = capture.failures.clone();
    failures.sort();
    payload.insert("failures".to_owned(), failures.join("\n"));
    payload.insert("agent_summary".to_owned(), capture.agent_summary.clone());
    let episode = store.add(NewMemory {
        memory_type: "episodic".to_owned(),
        name: format!("run {}", capture.run_id),
        content: summary,
        task: capture.task.clone(),
        run_id: capture.run_id.clone(),
        outcome: capture.outcome.clone(),
        links: capture.links.clone(),
        recorded_at: capture.recorded_at.clone(),
        confirm: false,
        pattern: false,
        intent_kind: String::new(),
        payload,
        provenance_kind: ProvenanceKind::Generated,
        method: "run-record".to_owned(),
        confidence_millis: 700,
    })?;
    let mut records = vec![episode];
    if capture.outcome == "failed" || !capture.failures.is_empty() {
        let mut lesson_payload = BTreeMap::new();
        lesson_payload.insert(
            "context_pack_id".to_owned(),
            capture.context_pack_id.clone(),
        );
        lesson_payload.insert("run_id".to_owned(), capture.run_id.clone());
        let failure_text = if failures.is_empty() {
            "outcome failed".to_owned()
        } else {
            failures.join("\n")
        };
        let lesson = store.add(NewMemory {
            memory_type: "lesson".to_owned(),
            name: format!("lesson {}", capture.run_id),
            content: format!(
                "lesson: {failure_text}\ntask: {}\nrun: {}\noutcome: failed\n",
                capture.task, capture.run_id
            ),
            task: capture.task.clone(),
            run_id: capture.run_id.clone(),
            outcome: "failed".to_owned(),
            links: capture.links.clone(),
            recorded_at: capture.recorded_at.clone(),
            confirm: false,
            pattern: false,
            intent_kind: String::new(),
            payload: lesson_payload,
            provenance_kind: ProvenanceKind::Inferred,
            method: "failure-lesson".to_owned(),
            confidence_millis: 400,
        })?;
        records.push(lesson);
    }
    if capture.outcome == "completed" {
        records.push(record_pattern(store, capture)?);
    }
    records.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(records)
}

/// Records a successful context-pack shape. It stays proposed.
pub fn record_pattern(
    store: &MemoryStore,
    capture: &RunCapture,
) -> Result<MemoryRecord, MemoryError> {
    let mut shape = capture.shape.clone();
    shape.sort();
    shape.dedup();
    let mut payload = BTreeMap::new();
    payload.insert(
        "context_pack_id".to_owned(),
        capture.context_pack_id.clone(),
    );
    payload.insert("shape".to_owned(), shape.join(","));
    payload.insert("intent_kind".to_owned(), capture.intent_kind.clone());
    store.add(NewMemory {
        memory_type: "procedural".to_owned(),
        name: format!("context pattern {}", capture.intent_kind),
        content: format!(
            "context pattern\nintent: {}\nshape: {}\ntask: {}\npack: {}\n",
            capture.intent_kind,
            shape.join(","),
            capture.task,
            capture.context_pack_id
        ),
        task: capture.task.clone(),
        run_id: String::new(),
        outcome: "completed".to_owned(),
        links: capture.links.clone(),
        recorded_at: capture.recorded_at.clone(),
        confirm: false,
        pattern: true,
        intent_kind: capture.intent_kind.clone(),
        payload,
        provenance_kind: ProvenanceKind::Generated,
        method: "context-pattern".to_owned(),
        confidence_millis: 700,
    })
}
