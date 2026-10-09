//! Knowledge-engine evaluation: project profiles and the scanned graph.
//!
//! Each case copies its fixture to a temporary directory, so the committed fixture is never
//! scanned in place. Metrics are integer fractions. A case passes only when the observed set
//! equals the oracle.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use agentos_core::fs::{FileSystem, RealFs};
use agentos_knowledge::{KnowledgeStore, detect, scan};
use serde::Deserialize;
use serde_json::Value;

use crate::{CaseResult, EvalError, EvalReport};

/// Runs every case in `cases_dir`. `fixtures_dir` is the workspace `fixtures/` directory.
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
        if case.oracle.trim().is_empty() {
            return Err(invalid("oracle must not be empty".to_owned()));
        }
        if case.description.trim().is_empty() {
            return Err(invalid("description must not be empty".to_owned()));
        }
        if !ids.insert(case.id.clone()) {
            return Err(invalid(format!("duplicate id {:?}", case.id)));
        }
        cases.push(execute(&case, fixtures_dir));
    }
    Ok(EvalReport {
        area: "knowledge".to_owned(),
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
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Kind {
    Profile,
    Graph,
}

impl Kind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Profile => "profile",
            Self::Graph => "graph",
        }
    }
}

fn execute(case: &Case, fixtures_dir: &Path) -> CaseResult {
    let (passed, detail) = match case.kind {
        Kind::Profile => profile_case(case, fixtures_dir),
        Kind::Graph => graph_case(case, fixtures_dir),
    };
    CaseResult {
        id: case.id.clone(),
        kind: case.kind.as_str().to_owned(),
        description: case.description.clone(),
        passed,
        detail,
    }
}

fn profile_case(case: &Case, fixtures_dir: &Path) -> (bool, String) {
    let fixture = fixtures_dir.join(&case.fixture);
    let golden = match read_json(&fixture.join(&case.oracle)) {
        Ok(value) => value,
        Err(err) => return (false, err),
    };
    let copy = match TempCopy::of(&fixture) {
        Ok(copy) => copy,
        Err(err) => return (false, err),
    };
    let first = match detect(copy.path()) {
        Ok(profile) => profile,
        Err(err) => return (false, err.to_string()),
    };
    let second = match detect(copy.path()) {
        Ok(profile) => profile,
        Err(err) => return (false, err.to_string()),
    };
    let actual = serde_json::to_value(&first).expect("profile serializes");
    let again = serde_json::to_value(&second).expect("profile serializes");
    let stable = actual == again;
    let matches = actual == golden;
    let class = actual["classification"].as_str().unwrap_or("?");
    let maturity = actual["maturity"].as_str().unwrap_or("?");
    let confidence = actual["confidence_millis"].as_u64().unwrap_or(0);
    let uncertain = actual["uncertain"].as_bool().unwrap_or(false);
    let mut detail = format!(
        "{class}/{maturity} confidence {confidence} uncertain {uncertain}; stable {stable}"
    );
    if !matches {
        detail.push_str("; profile differs from oracle");
    }
    (matches && stable, detail)
}

fn graph_case(case: &Case, fixtures_dir: &Path) -> (bool, String) {
    let fixture = fixtures_dir.join(&case.fixture);
    let golden = match read_json(&fixture.join(&case.oracle)) {
        Ok(value) => value,
        Err(err) => return (false, err),
    };
    let expected_nodes = keys(&golden["nodes"], |item| {
        triple(item, "id", "node_type", "provenance_kind")
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
    let copy = match TempCopy::of(&fixture) {
        Ok(copy) => copy,
        Err(err) => return (false, err),
    };
    if let Err(err) = scan(copy.path()) {
        return (false, err.to_string());
    }
    let store = match KnowledgeStore::open(copy.path()) {
        Ok(store) => store,
        Err(err) => return (false, err.to_string()),
    };
    let nodes = match store.nodes() {
        Ok(nodes) => nodes,
        Err(err) => return (false, err.to_string()),
    };
    let edges = match store.edges() {
        Ok(edges) => edges,
        Err(err) => return (false, err.to_string()),
    };
    let provenance_ok = store.provenance_complete().unwrap_or(false);
    let before: BTreeSet<(String, String)> = nodes
        .iter()
        .map(|node| (node.id.clone(), node.fingerprint.clone()))
        .collect();
    let actual_nodes: BTreeSet<String> = nodes
        .iter()
        .map(|node| {
            format!(
                "{}\t{}\t{}",
                node.id,
                node.node_type.as_str(),
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
    drop(store);
    let node_score = score(&expected_nodes, &actual_nodes);
    let edge_score = score(&expected_edges, &actual_edges);
    let rescan = match scan(copy.path()) {
        Ok(report) => report,
        Err(err) => return (false, err.to_string()),
    };
    let rescan_clean = rescan.changed == 0 && rescan.rows_written == 0;
    let db = copy.path().join(".agentos/agentos.db");
    if let Err(err) = fs::remove_file(&db) {
        return (false, format!("cannot delete {}: {err}", db.display()));
    }
    if let Err(err) = scan(copy.path()) {
        return (false, err.to_string());
    }
    let rebuilt = match KnowledgeStore::open(copy.path()) {
        Ok(store) => store.nodes(),
        Err(err) => return (false, err.to_string()),
    };
    let after = match rebuilt {
        Ok(nodes) => nodes
            .into_iter()
            .map(|node| (node.id, node.fingerprint))
            .collect::<BTreeSet<_>>(),
        Err(err) => return (false, err.to_string()),
    };
    let fingerprints_match = before == after;
    let passed = node_score.exact()
        && edge_score.exact()
        && provenance_ok
        && rescan_clean
        && fingerprints_match;
    let mut detail = format!(
        "nodes recall {} precision {}; edges recall {} precision {}; provenance {}; rescan writes {}; rebuild fingerprints {}",
        node_score.recall(),
        node_score.precision(),
        edge_score.recall(),
        edge_score.precision(),
        if provenance_ok {
            "complete"
        } else {
            "incomplete"
        },
        rescan.rows_written,
        if fingerprints_match {
            "match"
        } else {
            "differ"
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
    (passed, detail)
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
        .take(4)
        .map(String::as_str)
        .collect();
    let extra: Vec<&str> = actual
        .difference(expected)
        .take(4)
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

fn triple(value: &Value, a: &str, b: &str, c: &str) -> String {
    format!(
        "{}\t{}\t{}",
        value[a].as_str().unwrap_or(""),
        value[b].as_str().unwrap_or(""),
        value[c].as_str().unwrap_or("")
    )
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
            "agentos-knowledge-eval-{}-{}",
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
        // Derived index. A domain scan leaves rows that software `scan` preserves, so copying
        // them would make the graph case compare domain nodes against the software oracle.
        if name == ".agentos" {
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

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::run;

    struct RemoveDir<'a>(&'a Path);

    impl Drop for RemoveDir<'_> {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(self.0);
        }
    }

    #[test]
    fn derived_domain_index_is_excluded_from_the_graph_oracle() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let fixture = root.join("fixtures/python-fastapi");
        let state = fixture.join(".agentos");
        let _cleanup = RemoveDir(&state);
        let _ = std::fs::remove_dir_all(&state);
        agentos_knowledge::domain_scan(&fixture).expect("domain scan");
        let report = run(&root.join("evals/knowledge"), &root.join("fixtures")).expect("eval");
        assert!(report.is_ok(), "{report:?}");
        let case = report
            .cases
            .iter()
            .find(|case| case.id == "python-fastapi")
            .expect("python-fastapi case");
        assert!(case.passed, "{}", case.detail);
        assert!(
            case.detail.contains("nodes recall 54/54 precision 54/54"),
            "{}",
            case.detail
        );
    }
}
