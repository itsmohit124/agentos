//! Unified evaluation dashboard.
//!
//! `agentos evaluate` with no area runs every existing suite and compares the measured
//! results with `evals/thresholds.json`. Those thresholds are the recorded baseline, not a
//! target chosen ahead of the measurement.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use agentos_skills::builtin;
use serde::{Deserialize, Serialize};

use crate::{EvalError, EvalReport};

/// Areas printed by the dashboard, in a stable order.
///
/// `docs/spec/EVALUATION.md` names knowledge, DDD, context, skills, feedback, and adapters.
/// Workflow, architecture, and memory already have suites, so they are included too.
pub const AREAS: &[&str] = &[
    "knowledge",
    "ddd",
    "context",
    "skills",
    "workflow",
    "feedback",
    "architecture",
    "memory",
    "adapters",
];

/// One area on the dashboard.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AreaRow {
    /// Area name.
    pub area: String,
    /// Passing cases.
    pub passed: usize,
    /// Failing cases.
    pub failed: usize,
    /// All cases.
    pub total: usize,
    /// Whether this area met its measured threshold.
    pub ok: bool,
    /// Measured values used for the threshold check. Keys are sorted.
    pub measured: BTreeMap<String, String>,
}

/// The dashboard `agentos evaluate` prints when no area is given.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Dashboard {
    /// `PASS` or `FAIL`.
    pub overall: String,
    /// True when every area passed its threshold.
    pub ok: bool,
    /// One row per area, in [`AREAS`] order.
    pub areas: Vec<AreaRow>,
    /// Threshold file that was read.
    pub thresholds_file: PathBuf,
}

/// Thresholds derived from a measured run of the committed suites.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Thresholds {
    /// How the numbers were obtained.
    pub derivation: String,
    /// Minimum passing cases per area. A suite that passes fewer fails the dashboard.
    pub areas: BTreeMap<String, AreaThreshold>,
    /// Context ceilings taken from the measured case files.
    pub context: ContextThreshold,
}

/// Floor for one area.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AreaThreshold {
    /// Passing cases measured when this file was written.
    pub min_passed: usize,
}

/// Context ceilings. Both come from the case files filled in Phase 4.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ContextThreshold {
    /// Highest `max_tokens` recorded on a context case.
    pub max_observed_tokens: u32,
    /// Highest irrelevant-token ratio the cases allow.
    pub max_irrelevant_ratio: f64,
}

/// Runs every suite under `evals_dir` and applies `evals_dir/thresholds.json`.
pub fn run(evals_dir: &Path, fixtures_dir: &Path) -> Result<Dashboard, EvalError> {
    let thresholds_file = evals_dir.join("thresholds.json");
    let text = fs::read_to_string(&thresholds_file).map_err(|err| {
        EvalError::Failed(format!("cannot read {}: {err}", thresholds_file.display()))
    })?;
    let thresholds: Thresholds = serde_json::from_str(&text).map_err(|err| {
        EvalError::Failed(format!("invalid {}: {err}", thresholds_file.display()))
    })?;
    let mut areas = Vec::with_capacity(AREAS.len());
    for area in AREAS {
        let report = run_area(area, evals_dir, fixtures_dir)?;
        areas.push(row(area, &report, &thresholds));
    }
    let ok = areas.iter().all(|area| area.ok);
    Ok(Dashboard {
        overall: if ok { "PASS" } else { "FAIL" }.to_owned(),
        ok,
        areas,
        thresholds_file,
    })
}

/// Text form of the dashboard.
pub fn render(dashboard: &Dashboard) -> String {
    let mut out = String::from("AgentOS evaluation\n");
    for area in &dashboard.areas {
        let mark = if area.ok { "PASS" } else { "FAIL" };
        out.push_str(&format!(
            "  {:<14} {mark:<4} {}/{}",
            area.area, area.passed, area.total
        ));
        if let Some(tokens) = area.measured.get("max_tokens") {
            out.push_str(&format!("  max_tokens {tokens}"));
        }
        if let Some(irrelevant) = area.measured.get("irrelevant_tokens") {
            out.push_str(&format!("  irrelevant_tokens {irrelevant}"));
        }
        out.push('\n');
    }
    out.push_str(&format!("Overall: {}\n", dashboard.overall));
    out
}

fn run_area(area: &str, evals_dir: &Path, fixtures_dir: &Path) -> Result<EvalReport, EvalError> {
    let cases = evals_dir.join(area);
    match area {
        "knowledge" => crate::knowledge::run(&cases, fixtures_dir),
        "ddd" => crate::ddd::run(&cases, fixtures_dir),
        "context" => crate::context::run(&cases, fixtures_dir),
        "skills" => crate::skills::run(&cases, fixtures_dir, builtin::location()),
        "workflow" => crate::workflow::run(&cases, fixtures_dir),
        "feedback" => crate::feedback::run(&cases, fixtures_dir),
        "architecture" => crate::architecture::run(&cases, fixtures_dir),
        "memory" => crate::memory::run(&cases, fixtures_dir),
        "adapters" => crate::adapters::run(&cases, fixtures_dir),
        other => Err(EvalError::Failed(format!("unknown dashboard area {other}"))),
    }
}

fn row(area: &str, report: &EvalReport, thresholds: &Thresholds) -> AreaRow {
    let mut measured = BTreeMap::new();
    measured.insert("passed".to_owned(), report.passed().to_string());
    measured.insert("failed".to_owned(), report.failed().to_string());
    let mut ok = report.is_ok();
    if let Some(floor) = thresholds.areas.get(area) {
        measured.insert("min_passed".to_owned(), floor.min_passed.to_string());
        if report.passed() < floor.min_passed {
            ok = false;
        }
    } else {
        ok = false;
        measured.insert("min_passed".to_owned(), "missing".to_owned());
    }
    if area == "context" {
        let max_tokens = report
            .cases
            .iter()
            .filter_map(|case| field_u32(&case.detail, "tokens "))
            .max()
            .unwrap_or(0);
        let irrelevant: u32 = report
            .cases
            .iter()
            .filter_map(|case| field_u32(&case.detail, "irrelevant_tokens "))
            .sum();
        measured.insert("max_tokens".to_owned(), max_tokens.to_string());
        measured.insert("irrelevant_tokens".to_owned(), irrelevant.to_string());
        if max_tokens > thresholds.context.max_observed_tokens {
            ok = false;
        }
        if irrelevant > 0 && thresholds.context.max_irrelevant_ratio == 0.0 {
            ok = false;
        }
    }
    AreaRow {
        area: area.to_owned(),
        passed: report.passed(),
        failed: report.failed(),
        total: report.cases.len(),
        ok,
        measured,
    }
}

fn field_u32(detail: &str, label: &str) -> Option<u32> {
    let rest = detail.split(label).nth(1)?;
    let number: String = rest.chars().take_while(|ch| ch.is_ascii_digit()).collect();
    number.parse().ok()
}
