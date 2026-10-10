//! Context evaluation: required-context recall, exclusions, tokens, and reproducibility.
//!
//! Thresholds `max_tokens` and `max_irrelevant_ratio` are checked only when the case sets
//! them. They are filled from measured packs, not invented ahead of the baseline.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use agentos_context::{ContextConfig, NullSemantic, SqliteView, build, render_json};
use agentos_knowledge::{KnowledgeStore, domain_scan, scan};

use crate::baseline::{self, ContextCase};
use crate::{CaseResult, EvalError, EvalReport};

/// Runs every case in `cases_dir`.
pub fn run(cases_dir: &Path, fixtures_dir: &Path) -> Result<EvalReport, EvalError> {
    let cases = baseline::all_cases(cases_dir)?;
    let mut results = Vec::with_capacity(cases.len());
    for case in &cases {
        results.push(execute(case, fixtures_dir));
    }
    Ok(EvalReport {
        area: "context".to_owned(),
        cases_dir: cases_dir.to_path_buf(),
        cases: results,
    })
}

fn execute(case: &ContextCase, fixtures_dir: &Path) -> CaseResult {
    let (passed, detail) = match evaluate(case, fixtures_dir) {
        Ok(detail) => (detail.passed, detail.text),
        Err(err) => (false, err),
    };
    CaseResult {
        id: case.id.clone(),
        kind: "context".to_owned(),
        description: case.description.clone(),
        passed,
        detail,
    }
}

struct Detail {
    passed: bool,
    text: String,
}

fn evaluate(case: &ContextCase, fixtures_dir: &Path) -> Result<Detail, String> {
    let fixture = fixtures_dir.join(&case.fixture);
    if !fixture.is_dir() {
        return Err(format!("fixture {} is missing", fixture.display()));
    }
    let copy = TempCopy::of(&fixture)?;
    scan(copy.path()).map_err(|err| err.to_string())?;
    domain_scan(copy.path()).map_err(|err| err.to_string())?;
    let store = KnowledgeStore::open(copy.path()).map_err(|err| err.to_string())?;
    let view = SqliteView::new(&store);
    let config = ContextConfig::default();
    let started = Instant::now();
    let pack = build(&view, &case.task, &config, &NullSemantic).map_err(|err| err.to_string())?;
    let latency_us = u128_to_u64(started.elapsed().as_micros());
    let first =
        render_json(&view, &case.task, &config, &NullSemantic).map_err(|err| err.to_string())?;
    let second =
        render_json(&view, &case.task, &config, &NullSemantic).map_err(|err| err.to_string())?;
    let reproducible = first == second;

    let selected: Vec<&str> = pack
        .selected_nodes
        .iter()
        .map(|node| node.id.as_str())
        .collect();
    let missing: Vec<&str> = case
        .must_include
        .iter()
        .filter(|id| !selected.contains(&id.as_str()))
        .map(String::as_str)
        .collect();
    let forbidden: Vec<String> = pack
        .selected_nodes
        .iter()
        .filter(|node| {
            case.must_exclude
                .iter()
                .any(|banned| excluded_hit(&node.id, banned))
        })
        .map(|node| node.id.clone())
        .collect();
    let fragment_sources: Vec<&str> = pack
        .selected_fragments
        .iter()
        .map(|fragment| fragment.source_ref.as_str())
        .collect();
    let missing_fragments: Vec<&str> = case
        .required_fragments
        .iter()
        .filter(|path| !fragment_sources.contains(&path.as_str()))
        .map(String::as_str)
        .collect();

    let mut irrelevant_tokens = 0u32;
    let mut irrelevant_nodes = 0usize;
    for node in &pack.selected_nodes {
        if is_irrelevant(&node.id, &case.expected_contexts) {
            irrelevant_nodes += 1;
            irrelevant_tokens = irrelevant_tokens.saturating_add(node.token_estimate);
        }
    }
    for fragment in &pack.selected_fragments {
        if is_irrelevant(&fragment.node_id, &case.expected_contexts) {
            irrelevant_tokens = irrelevant_tokens.saturating_add(fragment.token_estimate);
        }
    }
    let selected_count = pack.selected_nodes.len();
    let relevant_nodes = selected_count.saturating_sub(irrelevant_nodes);
    let recall_hit = case.must_include.len() - missing.len();
    let fragment_hit = case.required_fragments.len() - missing_fragments.len();
    let used = pack.budget.used;
    let ratio_ok = case.max_irrelevant_ratio.is_none_or(|max| {
        if used == 0 {
            irrelevant_tokens == 0
        } else {
            f64::from(irrelevant_tokens) / f64::from(used) <= max + 1e-9
        }
    });
    let tokens_ok = case.max_tokens.is_none_or(|max| used <= max);
    let passed = missing.is_empty()
        && forbidden.is_empty()
        && missing_fragments.is_empty()
        && reproducible
        && ratio_ok
        && tokens_ok
        && !first.contains("timestamp");

    let mut text = format!(
        "recall {recall_hit}/{}; fragments {fragment_hit}/{}; precision {relevant_nodes}/{selected_count}; tokens {used}; irrelevant_tokens {irrelevant_tokens}; irrelevant_ratio {irrelevant_tokens}/{used}; latency_us {latency_us}; {}",
        case.must_include.len(),
        case.required_fragments.len(),
        if reproducible {
            "reproducible"
        } else {
            "NOT reproducible"
        }
    );
    if !missing.is_empty() {
        text.push_str(&format!("; missing {missing:?}"));
    }
    if !forbidden.is_empty() {
        text.push_str(&format!("; forbidden {forbidden:?}"));
    }
    if !missing_fragments.is_empty() {
        text.push_str(&format!("; missing_fragments {missing_fragments:?}"));
    }
    if !tokens_ok {
        text.push_str("; over max_tokens");
    }
    if !ratio_ok {
        text.push_str("; over max_irrelevant_ratio");
    }
    Ok(Detail { passed, text })
}

/// A bounded-context id in `must_exclude` also rejects every node in that package.
fn excluded_hit(selected_id: &str, banned: &str) -> bool {
    if selected_id == banned {
        return true;
    }
    let Some(rest) = banned.strip_prefix("domain.") else {
        return false;
    };
    if !banned.contains(".bounded_context.") {
        return false;
    }
    let Some(package) = rest.split('.').next() else {
        return false;
    };
    package_of(selected_id) == Some(package)
}

fn is_irrelevant(id: &str, expected: &[String]) -> bool {
    let Some(package) = package_of(id) else {
        return false;
    };
    !expected.iter().any(|name| name == package)
}

fn package_of(id: &str) -> Option<&str> {
    if let Some(rest) = id.strip_prefix("domain.") {
        return rest.split('.').next();
    }
    if let Some(rest) = id.strip_prefix("software.app.") {
        return rest.split('.').next();
    }
    if let Some(rest) = id.strip_prefix("quality.tests.test_") {
        return rest.split('.').next();
    }
    None
}

fn u128_to_u64(value: u128) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

struct TempCopy {
    path: PathBuf,
}

impl TempCopy {
    fn of(source: &Path) -> Result<Self, String> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "agentos-context-eval-{}-{}",
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
