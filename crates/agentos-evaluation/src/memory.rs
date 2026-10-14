//! Memory evaluation. Cases are deterministic and do not call a model or the network.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use agentos_context::{
    ContextConfig, MemoryView, NullSemantic, build, build_with_memory, render_json,
    render_json_with_memory,
};
use agentos_core::fs::{FileSystem, RealFs};
use agentos_knowledge::{KnowledgeStore, ProvenanceKind, domain_scan, scan};
use agentos_memory::{
    FingerprintIndex, MemoryLink, MemoryStore, NewMemory, RunCapture, record_pattern, record_run,
};
use serde::Deserialize;

use crate::{CaseResult, EvalError, EvalReport};

const AT: &str = "1970-01-01T00:00:00Z";

/// Runs every case in `cases_dir`.
pub fn run(cases_dir: &Path, fixtures_dir: &Path) -> Result<EvalReport, EvalError> {
    let fs = RealFs;
    if !fs.is_dir(cases_dir) {
        return Err(EvalError::NoCases(cases_dir.to_path_buf()));
    }
    let mut files: Vec<PathBuf> = fs
        .read_dir(cases_dir)?
        .into_iter()
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    files.sort();
    if files.is_empty() {
        return Err(EvalError::NoCases(cases_dir.to_path_buf()));
    }
    let mut cases = Vec::new();
    for path in files {
        let text = fs.read_to_string(&path)?;
        let case: Case = serde_json::from_str(&text).map_err(|err| EvalError::InvalidCase {
            path: path.clone(),
            message: err.to_string(),
        })?;
        let stem = path
            .file_stem()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        if case.id != stem {
            return Err(EvalError::InvalidCase {
                path,
                message: format!("id {:?} must equal the file name {stem:?}", case.id),
            });
        }
        if case.description.trim().is_empty() {
            return Err(EvalError::InvalidCase {
                path,
                message: "description must not be empty".to_owned(),
            });
        }
        cases.push(execute(&case, fixtures_dir));
    }
    Ok(EvalReport {
        area: "memory".to_owned(),
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
    #[serde(default = "default_task")]
    task: String,
    #[serde(default = "default_later")]
    later_task: String,
    #[serde(default = "default_failure")]
    failure: String,
    #[serde(default = "default_unrelated")]
    unrelated_task: String,
    #[serde(default = "default_fixture")]
    fixture: String,
}

fn default_task() -> String {
    "Add retry support to payment webhook processing".to_owned()
}

fn default_later() -> String {
    "Add idempotency to payment webhook processing".to_owned()
}

fn default_failure() -> String {
    "missing idempotency".to_owned()
}

fn default_unrelated() -> String {
    "Rename the cooking recipe parser".to_owned()
}

fn default_fixture() -> String {
    "python-fastapi".to_owned()
}

fn execute(case: &Case, fixtures_dir: &Path) -> CaseResult {
    let (passed, detail) = match run_case(case, fixtures_dir) {
        Ok(detail) => (true, detail),
        Err(err) => (false, err),
    };
    CaseResult {
        id: case.id.clone(),
        kind: case.kind.clone(),
        description: case.description.clone(),
        passed,
        detail,
        trace: None,
    }
}

fn run_case(case: &Case, fixtures_dir: &Path) -> Result<String, String> {
    match case.kind.as_str() {
        "lesson" => lesson(case),
        "exclusion" => exclusion(case),
        "stale" => stale(case),
        "forget" => forget(case),
        "phase4" => phase4(case, fixtures_dir),
        "canonical" => canonical(case),
        "budget" => budget(case),
        "redaction" => redaction(case),
        "reproducible" => reproducible(case),
        "pattern" => pattern(case),
        other => Err(format!("unknown memory case kind {other}")),
    }
}

fn lesson(case: &Case) -> Result<String, String> {
    let dir = tempfile::tempdir().map_err(|err| err.to_string())?;
    let store = MemoryStore::open(dir.path()).map_err(|err| err.to_string())?;
    let records = record_run(&store, &failed_capture(case)).map_err(|err| err.to_string())?;
    let lesson = records
        .iter()
        .find(|record| record.memory_type == "lesson")
        .ok_or("no lesson recorded")?;
    if lesson.status != "proposed" || lesson.is_canonical() {
        return Err(format!("lesson status {}", lesson.status));
    }
    let pack = pack_with(&store, &fresh_index(), &case.later_task)?;
    let node = pack
        .selected_nodes
        .iter()
        .find(|node| node.id == lesson.id)
        .ok_or_else(|| format!("lesson {} was not selected", lesson.id))?;
    if !node.reason.starts_with("previous_failure:") {
        return Err(format!("reason {}", node.reason));
    }
    let trace = pack
        .retrieval_trace
        .entries
        .iter()
        .find(|entry| entry.id == lesson.id && entry.outcome == "selected")
        .ok_or("lesson has no selected trace")?;
    if !trace.reason.starts_with("previous_failure:") {
        return Err(format!("trace {}", trace.reason));
    }
    if !pack
        .selected_fragments
        .iter()
        .any(|fragment| fragment.content.contains(&case.failure))
    {
        return Err("context pack does not contain the lesson".to_owned());
    }
    Ok(format!(
        "status {} reason {} trace {}",
        lesson.status, node.reason, trace.reason
    ))
}

fn exclusion(case: &Case) -> Result<String, String> {
    let dir = tempfile::tempdir().map_err(|err| err.to_string())?;
    let store = MemoryStore::open(dir.path()).map_err(|err| err.to_string())?;
    let records = record_run(&store, &failed_capture(case)).map_err(|err| err.to_string())?;
    let lesson = records
        .iter()
        .find(|record| record.memory_type == "lesson")
        .ok_or("no lesson recorded")?;
    let pack = pack_with(&store, &fresh_index(), &case.unrelated_task)?;
    if pack.selected_nodes.iter().any(|node| node.id == lesson.id)
        || pack
            .selected_fragments
            .iter()
            .any(|fragment| fragment.node_id == lesson.id)
    {
        return Err("unrelated task selected the lesson".to_owned());
    }
    Ok(format!("excluded {}", lesson.id))
}

fn stale(case: &Case) -> Result<String, String> {
    let dir = tempfile::tempdir().map_err(|err| err.to_string())?;
    let store = MemoryStore::open(dir.path()).map_err(|err| err.to_string())?;
    let records = record_run(&store, &failed_capture(case)).map_err(|err| err.to_string())?;
    let lesson = records
        .iter()
        .find(|record| record.memory_type == "lesson")
        .ok_or("no lesson recorded")?;
    let fresh = pack_with(&store, &fresh_index(), &case.later_task)?;
    let fresh_score = fresh
        .selected_nodes
        .iter()
        .find(|node| node.id == lesson.id)
        .ok_or("fresh lesson was not selected")?
        .score;
    let mut changed = fresh_index();
    changed.insert("app/payments/service.py", "fp-changed");
    let stale_pack = pack_with(&store, &changed, &case.later_task)?;
    let node = stale_pack
        .selected_nodes
        .iter()
        .find(|node| node.id == lesson.id)
        .ok_or("stale lesson disappeared")?;
    if node.status != "stale" || node.score >= fresh_score || !node.reason.starts_with("stale:") {
        return Err(format!(
            "status {} score {} fresh {fresh_score} reason {}",
            node.status, node.score, node.reason
        ));
    }
    let shown = store
        .show(lesson.id.as_str(), &changed, AT)
        .map_err(|err| err.to_string())?;
    if shown.freshness != "stale" {
        return Err("stale lesson is not inspectable as stale".to_owned());
    }
    Ok(format!(
        "fresh_score {fresh_score} stale_score {} status {}",
        node.score, node.status
    ))
}

fn forget(case: &Case) -> Result<String, String> {
    let dir = tempfile::tempdir().map_err(|err| err.to_string())?;
    let store = MemoryStore::open(dir.path()).map_err(|err| err.to_string())?;
    let records = record_run(&store, &failed_capture(case)).map_err(|err| err.to_string())?;
    let lesson = records
        .iter()
        .find(|record| record.memory_type == "lesson")
        .ok_or("no lesson recorded")?;
    store
        .forget(&lesson.id, AT)
        .map_err(|err| err.to_string())?;
    let pack = pack_with(&store, &fresh_index(), &case.later_task)?;
    let present = pack
        .retrieval_trace
        .entries
        .iter()
        .any(|entry| entry.id == lesson.id || entry.id.starts_with(&format!("{}#", lesson.id)))
        || pack.selected_nodes.iter().any(|node| node.id == lesson.id);
    if present {
        return Err("forgotten lesson was retrieved".to_owned());
    }
    Ok(format!("forgotten {}", lesson.id))
}

fn phase4(case: &Case, fixtures_dir: &Path) -> Result<String, String> {
    let fixture = fixtures_dir.join(&case.fixture);
    if !fixture.is_dir() {
        return Err(format!("fixture {} is missing", fixture.display()));
    }
    let copy = TempCopy::of(&fixture)?;
    scan(copy.path()).map_err(|err| err.to_string())?;
    domain_scan(copy.path()).map_err(|err| err.to_string())?;
    let knowledge = KnowledgeStore::open(copy.path()).map_err(|err| err.to_string())?;
    let view = agentos_context::SqliteView::new(&knowledge);
    let config = ContextConfig::default();
    let plain =
        render_json(&view, &case.task, &config, &NullSemantic).map_err(|err| err.to_string())?;
    let with_memory = render_json_with_memory(&view, &case.task, &config, &NullSemantic, &[])
        .map_err(|err| err.to_string())?;
    if plain != with_memory {
        return Err("empty memory changed the context pack".to_owned());
    }
    if plain.contains("\"memory.") {
        return Err("empty memory pack contains a memory id".to_owned());
    }
    Ok("empty memory pack matches".to_owned())
}

fn canonical(case: &Case) -> Result<String, String> {
    let dir = tempfile::tempdir().map_err(|err| err.to_string())?;
    let store = MemoryStore::open(dir.path()).map_err(|err| err.to_string())?;
    let records = record_run(&store, &failed_capture(case)).map_err(|err| err.to_string())?;
    let lesson = records
        .iter()
        .find(|record| record.memory_type == "lesson")
        .ok_or("no lesson recorded")?;
    if lesson.status != "proposed" || lesson.is_canonical() {
        return Err(format!("unconfirmed status {}", lesson.status));
    }
    let knowledge = KnowledgeStore::open(dir.path()).map_err(|err| err.to_string())?;
    if knowledge
        .nodes()
        .map_err(|err| err.to_string())?
        .iter()
        .any(|node| node.id.starts_with("memory.") || node.status == "canonical")
    {
        return Err("memory became a canonical knowledge node".to_owned());
    }
    let confirmed = store
        .confirm(&lesson.id, "operator", AT)
        .map_err(|err| err.to_string())?;
    if confirmed.status != "confirmed" || confirmed.is_canonical() {
        return Err(format!("confirmed status {}", confirmed.status));
    }
    Ok("proposed then confirmed; never canonical".to_owned())
}

fn budget(case: &Case) -> Result<String, String> {
    let dir = tempfile::tempdir().map_err(|err| err.to_string())?;
    let store = MemoryStore::open(dir.path()).map_err(|err| err.to_string())?;
    let mut capture = failed_capture(case);
    capture.failures = vec![format!("{} {}", case.failure, "x".repeat(20_000))];
    record_run(&store, &capture).map_err(|err| err.to_string())?;
    let view = MemoryView::default();
    let config = ContextConfig::default();
    let baseline =
        build(&view, &case.later_task, &config, &NullSemantic).map_err(|err| err.to_string())?;
    let limited = ContextConfig {
        budget: Some(baseline.budget.used),
        cache_dir: None,
    };
    let candidates = store
        .candidates(&fresh_index(), AT)
        .map_err(|err| err.to_string())?;
    let pack = build_with_memory(
        &view,
        &case.later_task,
        &limited,
        &NullSemantic,
        &candidates,
    )
    .map_err(|err| err.to_string())?;
    if pack.budget.used > baseline.budget.used {
        return Err(format!(
            "used {} over budget {}",
            pack.budget.used, baseline.budget.used
        ));
    }
    if pack
        .selected_fragments
        .iter()
        .any(|fragment| fragment.kind == "memory")
    {
        return Err("memory fragment exceeded the budget and was still selected".to_owned());
    }
    if !pack.retrieval_trace.entries.iter().any(|entry| {
        entry.item_kind == "fragment"
            && entry.reason == "over_budget"
            && entry.id.starts_with("memory.")
    }) {
        return Err("memory fragment was not traced as over_budget".to_owned());
    }
    Ok(format!(
        "used {} budget {}",
        pack.budget.used, baseline.budget.used
    ))
}

fn redaction(case: &Case) -> Result<String, String> {
    let dir = tempfile::tempdir().map_err(|err| err.to_string())?;
    let store = MemoryStore::open(dir.path()).map_err(|err| err.to_string())?;
    let record = store
        .add(NewMemory {
            memory_type: "lesson".to_owned(),
            name: "Secret".to_owned(),
            content: format!("{} password=hunter2 AKIAIOSFODNN7EXAMPLE", case.failure),
            task: case.task.clone(),
            run_id: "run-secret".to_owned(),
            outcome: "failed".to_owned(),
            links: Vec::new(),
            recorded_at: AT.to_owned(),
            confirm: false,
            pattern: false,
            intent_kind: String::new(),
            payload: Default::default(),
            provenance_kind: ProvenanceKind::Declared,
            method: "human-add".to_owned(),
            confidence_millis: 1000,
        })
        .map_err(|err| err.to_string())?;
    let path = dir
        .path()
        .join(".agentos/memory")
        .join(format!("{}.json", record.id));
    let text = fs::read_to_string(path).map_err(|err| err.to_string())?;
    if text.contains("hunter2") || text.contains("AKIAIOSFODNN7EXAMPLE") {
        return Err("secret-like text was stored".to_owned());
    }
    if !text.contains("[REDACTED]") {
        return Err("redaction marker missing".to_owned());
    }
    Ok("redacted before persistence".to_owned())
}

fn reproducible(case: &Case) -> Result<String, String> {
    let dir = tempfile::tempdir().map_err(|err| err.to_string())?;
    let store = MemoryStore::open(dir.path()).map_err(|err| err.to_string())?;
    record_run(&store, &failed_capture(case)).map_err(|err| err.to_string())?;
    let view = MemoryView::default();
    let config = ContextConfig::default();
    let candidates = store
        .candidates(&fresh_index(), AT)
        .map_err(|err| err.to_string())?;
    let first =
        render_json_with_memory(&view, &case.later_task, &config, &NullSemantic, &candidates)
            .map_err(|err| err.to_string())?;
    let second =
        render_json_with_memory(&view, &case.later_task, &config, &NullSemantic, &candidates)
            .map_err(|err| err.to_string())?;
    if first != second {
        return Err("context pack was not reproducible".to_owned());
    }
    if first.contains("timestamp") {
        return Err("pack contains a timestamp".to_owned());
    }
    Ok("reproducible".to_owned())
}

fn pattern(case: &Case) -> Result<String, String> {
    let dir = tempfile::tempdir().map_err(|err| err.to_string())?;
    let store = MemoryStore::open(dir.path()).map_err(|err| err.to_string())?;
    let mut capture = failed_capture(case);
    capture.outcome = "completed".to_owned();
    capture.failures.clear();
    capture.intent_kind = "feature".to_owned();
    capture.shape = vec!["class".to_owned(), "invariant".to_owned()];
    let recorded = record_pattern(&store, &capture).map_err(|err| err.to_string())?;
    let fresh = pack_with(&store, &fresh_index(), &case.task)?;
    if !fresh
        .selected_nodes
        .iter()
        .any(|node| node.id == recorded.id && node.reason.starts_with("context_pattern:"))
    {
        return Err("fresh pattern was not selected".to_owned());
    }
    let mut changed = fresh_index();
    changed.insert("app/payments/service.py", "fp-changed");
    let stale = pack_with(&store, &changed, &case.task)?;
    if stale
        .selected_nodes
        .iter()
        .any(|node| node.id == recorded.id)
    {
        return Err("stale pattern was reused as fresh context".to_owned());
    }
    let trace = stale
        .retrieval_trace
        .entries
        .iter()
        .find(|entry| entry.id == recorded.id)
        .ok_or("stale pattern has no trace")?;
    if trace.reason != "stale_pattern" || trace.outcome != "excluded" {
        return Err(format!("trace {} {}", trace.outcome, trace.reason));
    }
    Ok(format!("pattern {} stale_pattern", recorded.id))
}

fn failed_capture(case: &Case) -> RunCapture {
    RunCapture {
        run_id: "run-1".to_owned(),
        task: case.task.clone(),
        outcome: "failed".to_owned(),
        context_pack_id: "pack-1".to_owned(),
        pack_ids: vec!["pack-1".to_owned()],
        agent_summary: "webhook handler failed".to_owned(),
        validation: "unit_test failed".to_owned(),
        failures: vec![case.failure.clone()],
        links: vec![MemoryLink {
            kind: "source".to_owned(),
            id: "app/payments/service.py".to_owned(),
            fingerprint: "fp-1".to_owned(),
        }],
        recorded_at: AT.to_owned(),
        intent_kind: "feature".to_owned(),
        shape: vec!["class".to_owned()],
    }
}

fn fresh_index() -> FingerprintIndex {
    let mut index = FingerprintIndex::new();
    index.insert("app/payments/service.py", "fp-1");
    index
}

fn pack_with(
    store: &MemoryStore,
    index: &FingerprintIndex,
    task: &str,
) -> Result<agentos_context::ContextPack, String> {
    let candidates = store.candidates(index, AT).map_err(|err| err.to_string())?;
    build_with_memory(
        &MemoryView::default(),
        task,
        &ContextConfig::default(),
        &NullSemantic,
        &candidates,
    )
    .map_err(|err| err.to_string())
}

struct TempCopy {
    path: PathBuf,
}

impl TempCopy {
    fn of(source: &Path) -> Result<Self, String> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "agentos-memory-eval-{}-{}",
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
        if name == ".agentos" || name == ".git" || name == ".venv" || name == "node_modules" {
            continue;
        }
        let target = to.join(&name);
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else if file_type.is_file() {
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}
