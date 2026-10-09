//! Naive context baselines.
//!
//! This module measures two strategies that do not retrieve anything: the token cost of the
//! whole fixture, and the token cost of the files a person listed on each golden case.
//! It does not rank nodes, build a Context Pack, or depend on the context engine.
//! Later token comparisons read `evals/baselines/context-baseline.json`.

use std::fs;
use std::path::{Path, PathBuf};

use agentos_knowledge::{list_sources, token_estimate};
use serde::{Deserialize, Serialize};

use crate::EvalError;

/// Directory name under the project root.
pub const BASELINE_DIR: &str = "evals/baselines";

/// File name of the context baseline.
pub const BASELINE_FILE: &str = "context-baseline.json";

/// How file tokens are counted. The same heuristic is used on knowledge fragments.
pub const TOKEN_HEURISTIC: &str = "one token per four characters, rounded up";

/// One golden task's naive file costs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BaselineCase {
    /// Case id.
    pub id: String,
    /// Task text.
    pub task: String,
    /// Manually chosen files, sorted.
    pub relevant_files: Vec<String>,
    /// Tokens in those files.
    pub relevant_file_tokens: u64,
    /// Tokens in the whole fixture. Repeated so each case stands alone.
    pub full_repository_tokens: u64,
}

/// Measured baseline for one fixture.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BaselineReport {
    /// Fixture directory name.
    pub fixture: String,
    /// Token heuristic name.
    pub token_heuristic: String,
    /// Indexable text files.
    pub file_count: usize,
    /// Tokens across those files.
    pub full_repository_tokens: u64,
    /// One row per golden case, sorted by id.
    pub cases: Vec<BaselineCase>,
}

/// Golden case fields the baseline reads. Extra engine fields are not accepted.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextCase {
    /// Case id. Must match the file stem.
    pub id: String,
    /// Always `context`.
    pub kind: String,
    /// What the case checks.
    pub description: String,
    /// Fixture directory name under `fixtures/`.
    pub fixture: String,
    /// Oracle name. Non-empty.
    pub oracle: String,
    /// Task text passed to the context engine later.
    pub task: String,
    /// Files a person would open. The engine must not read this as a retrieval hint.
    pub relevant_files: Vec<String>,
    /// Logical ids that must appear in the pack.
    pub must_include: Vec<String>,
    /// Logical ids that must not appear. A bounded-context id also rejects that package.
    pub must_exclude: Vec<String>,
    /// Fragment `source_ref` values that must be selected.
    pub required_fragments: Vec<String>,
    /// Bounded-context packages treated as relevant for the irrelevant-token ratio.
    pub expected_contexts: Vec<String>,
    /// Pack token ceiling. `null` until the baseline has been measured.
    pub max_tokens: Option<u32>,
    /// Irrelevant-token ratio ceiling, from 0 to 1. `null` until measured.
    pub max_irrelevant_ratio: Option<f64>,
}

/// Measures the fixture named by `fixture_dir` against `evals/context` under `project_root`.
pub fn measure(project_root: &Path, fixture_dir: &Path) -> Result<BaselineReport, EvalError> {
    let fixture_name = fixture_dir
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            EvalError::Failed(format!(
                "fixture path {} has no name",
                fixture_dir.display()
            ))
        })?
        .to_owned();
    if !fixture_dir.is_dir() {
        return Err(EvalError::Failed(format!(
            "fixture {} is not a directory",
            fixture_dir.display()
        )));
    }
    let cases = load_cases(&project_root.join("evals/context"), &fixture_name)?;
    let sources = list_sources(fixture_dir).map_err(|err| EvalError::Failed(err.to_string()))?;
    let full = sources
        .iter()
        .map(|file| {
            u64::from(token_estimate(
                std::str::from_utf8(&file.bytes).unwrap_or(""),
            ))
        })
        .sum();
    let mut rows = Vec::with_capacity(cases.len());
    for case in cases {
        let mut files = case.relevant_files.clone();
        files.sort();
        files.dedup();
        let mut relevant = 0u64;
        for relative in &files {
            let path = fixture_dir.join(relative);
            let text = fs::read_to_string(&path).map_err(|err| {
                EvalError::Failed(format!(
                    "case {} relevant file {}: {err}",
                    case.id,
                    path.display()
                ))
            })?;
            relevant += u64::from(token_estimate(&text));
        }
        rows.push(BaselineCase {
            id: case.id,
            task: case.task,
            relevant_files: files,
            relevant_file_tokens: relevant,
            full_repository_tokens: full,
        });
    }
    rows.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(BaselineReport {
        fixture: fixture_name,
        token_heuristic: TOKEN_HEURISTIC.to_owned(),
        file_count: sources.len(),
        full_repository_tokens: full,
        cases: rows,
    })
}

/// Pretty JSON plus a trailing newline. Stable for a given report.
pub fn render(report: &BaselineReport) -> Result<String, EvalError> {
    let mut text = serde_json::to_string_pretty(report)
        .map_err(|err| EvalError::Failed(format!("cannot encode baseline: {err}")))?;
    text.push('\n');
    Ok(text)
}

/// Writes [`BASELINE_FILE`] under the project root and returns the report.
pub fn write(project_root: &Path, fixture_dir: &Path) -> Result<BaselineReport, EvalError> {
    let report = measure(project_root, fixture_dir)?;
    let dir = project_root.join(BASELINE_DIR);
    fs::create_dir_all(&dir).map_err(|err| EvalError::Failed(err.to_string()))?;
    let path = dir.join(BASELINE_FILE);
    fs::write(&path, render(&report)?).map_err(|err| EvalError::Failed(err.to_string()))?;
    Ok(report)
}

/// Loads and checks context cases for one fixture, in file-name order.
pub fn load_cases(cases_dir: &Path, fixture: &str) -> Result<Vec<ContextCase>, EvalError> {
    if !cases_dir.is_dir() {
        return Err(EvalError::NoCases(cases_dir.to_path_buf()));
    }
    let mut paths: Vec<PathBuf> = fs::read_dir(cases_dir)
        .map_err(|err| EvalError::Failed(err.to_string()))?
        .map(|entry| {
            entry
                .map(|entry| entry.path())
                .map_err(|err| EvalError::Failed(err.to_string()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    paths.retain(|path| path.extension().is_some_and(|ext| ext == "json"));
    paths.sort();
    if paths.is_empty() {
        return Err(EvalError::NoCases(cases_dir.to_path_buf()));
    }
    let mut cases = Vec::new();
    for path in paths {
        let text = fs::read_to_string(&path).map_err(|err| EvalError::Failed(err.to_string()))?;
        let case: ContextCase =
            serde_json::from_str(&text).map_err(|err| EvalError::InvalidCase {
                path: path.clone(),
                message: err.to_string(),
            })?;
        let stem = path
            .file_stem()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        if case.id != stem {
            return Err(EvalError::InvalidCase {
                path: path.clone(),
                message: format!("id {:?} must equal the file name {stem:?}", case.id),
            });
        }
        if case.kind != "context" {
            return Err(EvalError::InvalidCase {
                path: path.clone(),
                message: format!("kind must be \"context\", found {:?}", case.kind),
            });
        }
        if case.oracle.trim().is_empty()
            || case.description.trim().is_empty()
            || case.task.trim().is_empty()
        {
            return Err(EvalError::InvalidCase {
                path,
                message: "oracle, description, and task must not be empty".to_owned(),
            });
        }
        if case.fixture == fixture {
            cases.push(case);
        }
    }
    if cases.is_empty() {
        return Err(EvalError::Failed(format!(
            "no context cases for fixture {fixture} in {}",
            cases_dir.display()
        )));
    }
    Ok(cases)
}
