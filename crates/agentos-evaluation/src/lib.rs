//! AgentOS deterministic evaluation harness.
//!
//! Evaluation cases are JSON files in `evals/<area>/`, one case per file, run in file-name order.
//! Every case names its fixture and its oracle; no LLM or network is involved, so the same inputs
//! always produce the same report.
//!
//! Implemented areas: [`skills`], [`knowledge`], [`ddd`]. Context cases are measured by
//! [`baseline`] before the context engine ranks them.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::Serialize;

pub mod baseline;
pub mod ddd;
pub mod knowledge;
pub mod skills;

/// Outcome of one evaluation case.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CaseResult {
    /// Case id (file stem, or `<file>/<sub-case>` for expanded cases).
    pub id: String,
    /// Case kind (e.g. `route`).
    pub kind: String,
    /// What the case checks.
    pub description: String,
    /// Whether the oracle accepted the result.
    pub passed: bool,
    /// What was observed (and, on failure, what was expected).
    pub detail: String,
}

/// Pass/total counts for one case kind.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct KindMetrics {
    /// Cases that passed.
    pub passed: usize,
    /// All cases.
    pub total: usize,
}

/// Results of one evaluation area.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EvalReport {
    /// Area name (e.g. `skills`).
    pub area: String,
    /// Directory the cases were read from.
    pub cases_dir: PathBuf,
    /// Results in run order.
    pub cases: Vec<CaseResult>,
}

impl EvalReport {
    /// Number of passing cases.
    pub fn passed(&self) -> usize {
        self.cases.iter().filter(|c| c.passed).count()
    }

    /// Number of failing cases.
    pub fn failed(&self) -> usize {
        self.cases.len() - self.passed()
    }

    /// Whether every case passed (and there was at least one).
    pub fn is_ok(&self) -> bool {
        !self.cases.is_empty() && self.failed() == 0
    }

    /// Pass/total per case kind, sorted by kind.
    pub fn by_kind(&self) -> BTreeMap<String, KindMetrics> {
        let mut out: BTreeMap<String, KindMetrics> = BTreeMap::new();
        for case in &self.cases {
            let m = out.entry(case.kind.clone()).or_default();
            m.total += 1;
            m.passed += usize::from(case.passed);
        }
        out
    }
}

/// Why an evaluation could not run (as opposed to a case failing).
#[derive(Debug, thiserror::Error)]
pub enum EvalError {
    /// The cases directory is missing or empty.
    #[error("no evaluation cases found in {}", .0.display())]
    NoCases(PathBuf),
    /// A file could not be read.
    #[error(transparent)]
    Fs(#[from] agentos_core::fs::FsError),
    /// A case file is malformed.
    #[error("invalid case file {}: {message}", .path.display())]
    InvalidCase {
        /// The file.
        path: PathBuf,
        /// What is wrong.
        message: String,
    },
    /// A baseline measurement could not be completed.
    #[error("{0}")]
    Failed(String),
}
