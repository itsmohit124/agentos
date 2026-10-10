//! Architecture evaluation: detector precision and recall against fixture ground truth.
//!
//! A case passes when every expected finding is present, no unexpected finding is present,
//! and a second analysis of the same copy produces the same findings. An import-edge oracle
//! names the importer in `from` and the imported file in `to`. The finding must list them in
//! that order. A reversed edge does not match. An empty expected set and an empty report is
//! agreement: precision and recall are `0/0`, false positives and false negatives are both
//! zero, and the case passes.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use agentos_core::fs::{FileSystem, RealFs};
use agentos_feedback::{ArchitectureConfig, ArchitectureFinding, analyze_project};
use serde::Deserialize;

use crate::{CaseResult, EvalError, EvalReport};

/// Runs every case in `cases_dir`.
pub fn run(cases_dir: &Path, fixtures_dir: &Path) -> Result<EvalReport, EvalError> {
    let fs = RealFs;
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
        if case.oracle.trim().is_empty() || case.description.trim().is_empty() {
            return Err(EvalError::InvalidCase {
                path,
                message: "oracle and description must not be empty".to_owned(),
            });
        }
        cases.push(execute(&case, fixtures_dir));
    }
    Ok(EvalReport {
        area: "architecture".to_owned(),
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
}

#[derive(Debug, Deserialize)]
struct Oracle {
    findings: Vec<Expected>,
}

#[derive(Debug, Deserialize)]
struct Expected {
    detector: String,
    path: String,
    #[serde(default)]
    from: Option<String>,
    #[serde(default)]
    to: Option<String>,
}

fn execute(case: &Case, fixtures_dir: &Path) -> CaseResult {
    let result = score(case, fixtures_dir);
    CaseResult {
        id: case.id.clone(),
        kind: case.kind.clone(),
        description: case.description.clone(),
        passed: result.is_ok(),
        detail: match result {
            Ok(detail) | Err(detail) => detail,
        },
    }
}

fn score(case: &Case, fixtures_dir: &Path) -> Result<String, String> {
    let fixture = fixtures_dir.join(&case.fixture);
    let oracle_path = fixture.join(&case.oracle);
    let oracle: Oracle = serde_json::from_str(
        &fs::read_to_string(&oracle_path)
            .map_err(|err| format!("cannot read {}: {err}", oracle_path.display()))?,
    )
    .map_err(|err| format!("invalid oracle {}: {err}", oracle_path.display()))?;
    let copied = TempCopy::of(&fixture)?;
    let config = ArchitectureConfig::default();
    let first = analyze_project(copied.path(), &config).map_err(|err| err.to_string())?;
    let second = analyze_project(copied.path(), &config).map_err(|err| err.to_string())?;
    let reproducible = first.findings == second.findings;
    let matched_expected = oracle
        .findings
        .iter()
        .filter(|expected| {
            first
                .findings
                .iter()
                .any(|finding| covers(finding, expected))
        })
        .count();
    let matched_findings = first
        .findings
        .iter()
        .filter(|finding| {
            oracle
                .findings
                .iter()
                .any(|expected| covers(finding, expected))
        })
        .count();
    let false_negatives = oracle.findings.len() - matched_expected;
    let false_positives = first.findings.len() - matched_findings;
    let detail = format!(
        "precision {matched_findings}/{} recall {matched_expected}/{} fp {false_positives} fn {false_negatives} reproducible {reproducible}",
        first.findings.len(),
        oracle.findings.len()
    );
    if !reproducible {
        return Err(format!("{detail}; second run differed"));
    }
    if false_positives != 0 || false_negatives != 0 {
        let missed: Vec<String> = oracle
            .findings
            .iter()
            .filter(|expected| {
                !first
                    .findings
                    .iter()
                    .any(|finding| covers(finding, expected))
            })
            .map(|expected| format!("{}:{}", expected.detector, expected.path))
            .collect();
        let extra: Vec<String> = first
            .findings
            .iter()
            .filter(|finding| {
                !oracle
                    .findings
                    .iter()
                    .any(|expected| covers(finding, expected))
            })
            .map(|finding| finding.id.clone())
            .collect();
        return Err(format!("{detail}; missed {missed:?}; extra {extra:?}"));
    }
    Ok(detail)
}

fn covers(finding: &ArchitectureFinding, expected: &Expected) -> bool {
    if finding.detector != expected.detector {
        return false;
    }
    let mentions_path = finding.paths.iter().any(|path| path == &expected.path);
    match (&expected.from, &expected.to) {
        (Some(from), Some(to)) => {
            mentions_path
                && finding.paths.first().map(String::as_str) == Some(from.as_str())
                && finding.paths.get(1).map(String::as_str) == Some(to.as_str())
        }
        (None, None) => mentions_path,
        _ => false,
    }
}

struct TempCopy {
    path: PathBuf,
}

impl TempCopy {
    fn of(source: &Path) -> Result<Self, String> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "agentos-architecture-eval-{}-{}",
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

#[cfg(test)]
mod tests {
    use agentos_feedback::ResultProvenance;

    use super::*;

    fn finding(paths: Vec<&str>) -> ArchitectureFinding {
        ArchitectureFinding {
            id: "unwanted_direction:test".to_owned(),
            detector: "unwanted_direction".to_owned(),
            summary: "direction".to_owned(),
            paths: paths.into_iter().map(str::to_owned).collect(),
            subjects: Vec::new(),
            evidence: "IMPORTS goes from an inner layer to an outer layer".to_owned(),
            provenance: ResultProvenance {
                kind: "inferred".to_owned(),
                source: String::new(),
                location: String::new(),
                extractor: "architecture".to_owned(),
                extractor_version: "1".to_owned(),
                method: "import-edge".to_owned(),
            },
        }
    }

    fn expected_edge() -> Expected {
        Expected {
            detector: "unwanted_direction".to_owned(),
            path: "app/orders/application/checkout.py".to_owned(),
            from: Some("app/orders/application/checkout.py".to_owned()),
            to: Some("app/orders/adapter/db.py".to_owned()),
        }
    }

    #[test]
    fn reversed_dependency_does_not_satisfy_the_expected_direction() {
        let expected = expected_edge();
        let forward = finding(vec![
            "app/orders/application/checkout.py",
            "app/orders/adapter/db.py",
        ]);
        let reversed = finding(vec![
            "app/orders/adapter/db.py",
            "app/orders/application/checkout.py",
        ]);
        assert!(covers(&forward, &expected));
        assert!(
            !covers(&reversed, &expected),
            "a reversed edge must not match"
        );
    }
}
