//! DDD evaluation: domain nodes, edges, findings, lifecycle, and generated markdown.
//!
//! A case passes only when the observed set equals the oracle. Precision and recall are
//! reported as integer fractions. No numeric target is applied beyond that exact match.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use agentos_core::fs::{FileSystem, RealFs};
use agentos_knowledge::{
    DomainProposer, FixedClock, KnowledgeStore, NullProposer, confirm_with, domain_scan_with,
    propose_with,
};
use serde::Deserialize;
use serde_json::Value;

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
    let mut ids = BTreeSet::new();
    for path in files {
        let text = fs.read_to_string(&path)?;
        let invalid = |message: String| EvalError::InvalidCase {
            path: path.clone(),
            message,
        };
        let case: Case = serde_json::from_str(&text).map_err(|err| invalid(err.to_string()))?;
        let stem = path
            .file_stem()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        if case.id != stem {
            return Err(invalid(format!(
                "id {:?} must equal the file name {stem:?}",
                case.id
            )));
        }
        if case.oracle.trim().is_empty() || case.description.trim().is_empty() {
            return Err(invalid(
                "oracle and description must not be empty".to_owned(),
            ));
        }
        if !ids.insert(case.id.clone()) {
            return Err(invalid(format!("duplicate id {:?}", case.id)));
        }
        cases.push(execute(&case, fixtures_dir));
    }
    Ok(EvalReport {
        area: "ddd".to_owned(),
        cases_dir: cases_dir.to_path_buf(),
        cases,
    })
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    id: String,
    kind: Kind,
    description: String,
    fixture: String,
    oracle: String,
    #[serde(default)]
    confirm: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Kind {
    Domain,
    Markdown,
}

impl Kind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Domain => "domain",
            Self::Markdown => "markdown",
        }
    }
}

fn execute(case: &Case, fixtures_dir: &Path) -> CaseResult {
    let (passed, detail) = match case.kind {
        Kind::Domain => domain_case(case, fixtures_dir),
        Kind::Markdown => markdown_case(case, fixtures_dir),
    };
    CaseResult {
        id: case.id.clone(),
        kind: case.kind.as_str().to_owned(),
        description: case.description.clone(),
        passed,
        detail,
        trace: None,
    }
}

fn domain_case(case: &Case, fixtures_dir: &Path) -> (bool, String) {
    let fixture = fixtures_dir.join(&case.fixture);
    let golden = match read_json(&fixture.join(&case.oracle)) {
        Ok(value) => value,
        Err(err) => return (false, err),
    };
    let expected_nodes = keys(&golden["nodes"], |item| {
        format!(
            "{}\t{}\t{}\t{}",
            item["id"].as_str().unwrap_or(""),
            item["node_type"].as_str().unwrap_or(""),
            item["status"].as_str().unwrap_or(""),
            item["provenance_kind"].as_str().unwrap_or("")
        )
    });
    let expected_edges = keys(&golden["edges"], |item| {
        format!(
            "{}\t{}\t{}\t{}",
            item["from"].as_str().unwrap_or(""),
            item["relation"].as_str().unwrap_or(""),
            item["to"].as_str().unwrap_or(""),
            item["provenance_kind"].as_str().unwrap_or("")
        )
    });
    let expected_findings = keys(&golden["findings"], |item| {
        format!(
            "{}\t{}",
            item["id"].as_str().unwrap_or(""),
            item["kind"].as_str().unwrap_or("")
        )
    });
    let copy = match TempCopy::of(&fixture) {
        Ok(copy) => copy,
        Err(err) => return (false, err),
    };
    let clock = FixedClock {
        instant: "eval".to_owned(),
    };
    if let Err(err) = domain_scan_with(copy.path(), &clock) {
        return (false, err.to_string());
    }
    let store = match KnowledgeStore::open(copy.path()) {
        Ok(store) => store,
        Err(err) => return (false, err.to_string()),
    };
    let nodes = match store.domain_nodes() {
        Ok(nodes) => nodes,
        Err(err) => return (false, err.to_string()),
    };
    let edges = match store.domain_edges() {
        Ok(edges) => edges,
        Err(err) => return (false, err.to_string()),
    };
    let findings = match store.domain_findings() {
        Ok(findings) => findings,
        Err(err) => return (false, err.to_string()),
    };
    let actual_nodes: BTreeSet<String> = nodes
        .iter()
        .map(|node| {
            format!(
                "{}\t{}\t{}\t{}",
                node.id,
                node.node_type.as_str(),
                node.status,
                node.provenance.kind.as_str()
            )
        })
        .collect();
    let actual_edges: BTreeSet<String> = edges
        .iter()
        .map(|edge| {
            format!(
                "{}\t{}\t{}\t{}",
                edge.from_id,
                edge.relation.as_str(),
                edge.to_id,
                edge.provenance.kind.as_str()
            )
        })
        .collect();
    let actual_findings: BTreeSet<String> = findings
        .iter()
        .map(|finding| format!("{}\t{}", finding.id, finding.kind))
        .collect();
    let provenance_ok = nodes.iter().all(|node| {
        node.provenance.is_complete()
            && !node.provenance.source.is_empty()
            && !node.provenance.location.is_empty()
            && node.confidence_millis > 0
            && node.payload.contains_key("evidence")
    }) && edges.iter().all(|edge| edge.provenance.is_complete());
    let confirmed = nodes
        .iter()
        .filter(|node| node.status == "confirmed")
        .count();
    let canonical = nodes
        .iter()
        .filter(|node| node.status == "canonical")
        .count();
    let node_score = score(&expected_nodes, &actual_nodes);
    let edge_score = score(&expected_edges, &actual_edges);
    let finding_score = score(&expected_findings, &actual_findings);
    let ambiguity = findings
        .iter()
        .filter(|finding| finding.kind == "terminology_ambiguity")
        .count();
    let passed = node_score.exact()
        && edge_score.exact()
        && finding_score.exact()
        && provenance_ok
        && confirmed == 0
        && canonical == 0;
    let mut detail = format!(
        "nodes recall {} precision {}; edges recall {} precision {}; findings recall {} precision {}; lifecycle confirmed {confirmed} canonical {canonical}; provenance {}; ambiguity {ambiguity}",
        node_score.recall(),
        node_score.precision(),
        edge_score.recall(),
        edge_score.precision(),
        finding_score.recall(),
        finding_score.precision(),
        if provenance_ok {
            "complete"
        } else {
            "incomplete"
        },
    );
    if !node_score.exact() {
        detail.push_str(&format!(
            "; node mismatch {}",
            mismatch(&expected_nodes, &actual_nodes)
        ));
    }
    if !edge_score.exact() {
        detail.push_str(&format!(
            "; edge mismatch {}",
            mismatch(&expected_edges, &actual_edges)
        ));
    }
    if !finding_score.exact() {
        detail.push_str(&format!(
            "; finding mismatch {}",
            mismatch(&expected_findings, &actual_findings)
        ));
    }
    (passed, detail)
}

fn markdown_case(case: &Case, fixtures_dir: &Path) -> (bool, String) {
    let Some(confirm_id) = case.confirm.as_deref() else {
        return (false, "markdown case requires confirm".to_owned());
    };
    let fixture = fixtures_dir.join(&case.fixture);
    let oracle = fixture.join(&case.oracle);
    let copy = match TempCopy::of(&fixture) {
        Ok(copy) => copy,
        Err(err) => return (false, err),
    };
    let clock = FixedClock {
        instant: "eval".to_owned(),
    };
    let proposer = NullProposer;
    if let Err(err) = domain_scan_with(copy.path(), &clock) {
        return (false, err.to_string());
    }
    if let Err(err) = propose_with(copy.path(), &proposer as &dyn DomainProposer, &clock) {
        return (false, err.to_string());
    }
    if let Err(err) = confirm_with(
        copy.path(),
        confirm_id,
        "operator",
        "operator confirmation",
        &clock,
    ) {
        return (false, err.to_string());
    }
    let actual_dir = copy.path().join(".agentos/knowledge/domain");
    match compare_markdown(&oracle, &actual_dir) {
        Ok(()) => (
            true,
            "markdown match; timestamps absent; confirmed and proposed sections present".to_owned(),
        ),
        Err(err) => (false, err),
    }
}

fn compare_markdown(oracle: &Path, actual: &Path) -> Result<(), String> {
    let mut names = BTreeSet::new();
    for dir in [oracle, actual] {
        let entries =
            fs::read_dir(dir).map_err(|err| format!("cannot read {}: {err}", dir.display()))?;
        for entry in entries {
            let entry = entry.map_err(|err| err.to_string())?;
            if entry.path().extension().is_some_and(|ext| ext == "md") {
                names.insert(entry.file_name().to_string_lossy().into_owned());
            }
        }
    }
    if names.is_empty() {
        return Err(format!("no markdown in {}", oracle.display()));
    }
    for name in names {
        let expected = fs::read_to_string(oracle.join(&name))
            .map_err(|err| format!("missing oracle {name}: {err}"))?;
        let got = fs::read_to_string(actual.join(&name))
            .map_err(|err| format!("missing generated {name}: {err}"))?;
        if expected.contains("CLOCK") || expected.contains("1970") {
            return Err(format!("{name} oracle contains a timestamp-like token"));
        }
        if got != expected {
            return Err(format!("{name} differs from the golden file"));
        }
    }
    Ok(())
}

struct Score {
    hit: usize,
    expected: usize,
    actual: usize,
}

impl Score {
    fn exact(&self) -> bool {
        self.hit == self.expected && self.hit == self.actual
    }

    fn recall(&self) -> String {
        format!("{}/{}", self.hit, self.expected)
    }

    fn precision(&self) -> String {
        format!("{}/{}", self.hit, self.actual)
    }
}

fn score(expected: &BTreeSet<String>, actual: &BTreeSet<String>) -> Score {
    Score {
        hit: expected.intersection(actual).count(),
        expected: expected.len(),
        actual: actual.len(),
    }
}

fn mismatch(expected: &BTreeSet<String>, actual: &BTreeSet<String>) -> String {
    let missing: Vec<&str> = expected
        .difference(actual)
        .take(6)
        .map(String::as_str)
        .collect();
    let extra: Vec<&str> = actual
        .difference(expected)
        .take(6)
        .map(String::as_str)
        .collect();
    format!("missing {missing:?} extra {extra:?}")
}

fn keys(value: &Value, map: impl Fn(&Value) -> String) -> BTreeSet<String> {
    value
        .as_array()
        .map(|items| items.iter().map(map).collect())
        .unwrap_or_default()
}

fn read_json(path: &Path) -> Result<Value, String> {
    let text =
        fs::read_to_string(path).map_err(|err| format!("cannot read {}: {err}", path.display()))?;
    serde_json::from_str(&text).map_err(|err| format!("invalid JSON {}: {err}", path.display()))
}

struct TempCopy {
    path: PathBuf,
}

impl TempCopy {
    fn of(source: &Path) -> Result<Self, String> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "agentos-ddd-eval-{}-{}",
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
        if name == ".agentos" || name == ".venv" || name == "node_modules" {
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
