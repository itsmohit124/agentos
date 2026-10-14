//! Run and plan files under `.agentos/runs/`.
//!
//! The directory is derived state. Deleting it drops run history. It does not
//! change the project source.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::WorkflowError;
use crate::plan::Plan;
use crate::run::RunRecord;

#[derive(Debug, Default, Serialize, Deserialize)]
struct IndexFile {
    plans: Vec<String>,
    runs: Vec<IndexRun>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct IndexRun {
    id: String,
    plan_id: String,
    state: String,
}

/// Directory that holds plans and runs for `root`.
pub fn runs_dir(root: &Path) -> PathBuf {
    root.join(".agentos/runs")
}

/// Writes `plan` to `.agentos/runs/plans/<id>.json`.
pub fn save_plan(root: &Path, plan: &Plan) -> Result<(), WorkflowError> {
    let dir = runs_dir(root);
    let plans = dir.join("plans");
    fs::create_dir_all(&plans).map_err(store)?;
    write_json(&plans.join(format!("{}.json", plan.id)), plan)?;
    let mut index = read_index(&dir)?;
    if !index.plans.contains(&plan.id) {
        index.plans.push(plan.id.clone());
        index.plans.sort();
    }
    write_index(&dir, &index)
}

/// Reads a plan by id.
pub fn load_plan(root: &Path, plan_id: &str) -> Result<Plan, WorkflowError> {
    let path = runs_dir(root).join("plans").join(format!("{plan_id}.json"));
    if !path.is_file() {
        return Err(WorkflowError::NotFound {
            kind: "plan",
            id: plan_id.to_owned(),
        });
    }
    read_json(&path)
}

/// Writes the run, its context packs, and the index entry.
pub fn save_run(root: &Path, run: &RunRecord) -> Result<(), WorkflowError> {
    let dir = runs_dir(root);
    let run_dir = dir.join(&run.id);
    fs::create_dir_all(run_dir.join("context")).map_err(store)?;
    write_json(&run_dir.join("run.json"), run)?;
    for step in &run.steps {
        if let Some(pack) = &step.context_pack {
            write_json(
                &run_dir.join("context").join(format!("{}.json", step.id)),
                pack,
            )?;
        }
    }
    let mut index = read_index(&dir)?;
    if let Some(entry) = index.runs.iter_mut().find(|entry| entry.id == run.id) {
        entry.state = run.state.as_str().to_owned();
        entry.plan_id = run.plan_id.clone();
    } else {
        index.runs.push(IndexRun {
            id: run.id.clone(),
            plan_id: run.plan_id.clone(),
            state: run.state.as_str().to_owned(),
        });
    }
    index.runs.sort_by(|left, right| left.id.cmp(&right.id));
    write_index(&dir, &index)
}

/// Reads a run by id.
pub fn load_run(root: &Path, run_id: &str) -> Result<RunRecord, WorkflowError> {
    let path = runs_dir(root).join(run_id).join("run.json");
    if !path.is_file() {
        return Err(WorkflowError::NotFound {
            kind: "run",
            id: run_id.to_owned(),
        });
    }
    read_json(&path)
}

/// Rebuilds `index.json` from plan and run files.
///
/// A missing or unreadable index is replaced. A run file that is not valid JSON is left
/// in place and reported by [`load_run`]; it is not invented back into the index.
pub fn recover_index(root: &Path) -> Result<usize, WorkflowError> {
    let dir = runs_dir(root);
    let mut index = IndexFile::default();
    let plans = dir.join("plans");
    if plans.is_dir() {
        let mut entries = fs::read_dir(&plans)
            .map_err(store)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(store)?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "json")
                && let Ok(plan) = read_json::<Plan>(&path)
            {
                index.plans.push(plan.id);
            }
        }
    }
    if dir.is_dir() {
        let mut entries = fs::read_dir(&dir)
            .map_err(store)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(store)?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let run_path = path.join("run.json");
            if let Ok(run) = read_json::<crate::run::RunRecord>(&run_path) {
                index.runs.push(IndexRun {
                    id: run.id,
                    plan_id: run.plan_id,
                    state: run.state.as_str().to_owned(),
                });
            }
        }
    }
    index.plans.sort();
    index.plans.dedup();
    index.runs.sort_by(|left, right| left.id.cmp(&right.id));
    let count = index.plans.len() + index.runs.len();
    if dir.exists() || count > 0 {
        write_index(&dir, &index)?;
    }
    Ok(count)
}

/// How many runs already exist for `plan_id`.
pub fn runs_for_plan(root: &Path, plan_id: &str) -> Result<usize, WorkflowError> {
    let index = read_index(&runs_dir(root))?;
    Ok(index
        .runs
        .iter()
        .filter(|entry| entry.plan_id == plan_id)
        .count())
}

fn read_index(dir: &Path) -> Result<IndexFile, WorkflowError> {
    let path = dir.join("index.json");
    if !path.is_file() {
        return Ok(IndexFile::default());
    }
    read_json(&path)
}

fn write_index(dir: &Path, index: &IndexFile) -> Result<(), WorkflowError> {
    fs::create_dir_all(dir).map_err(store)?;
    write_json(&dir.join("index.json"), index)
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), WorkflowError> {
    let mut text =
        serde_json::to_string_pretty(value).map_err(|err| WorkflowError::Store(err.to_string()))?;
    text.push('\n');
    fs::write(path, text).map_err(store)
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, WorkflowError> {
    let text = fs::read_to_string(path).map_err(store)?;
    serde_json::from_str(&text)
        .map_err(|err| WorkflowError::Store(format!("{}: {err}", path.display())))
}

fn store(err: std::io::Error) -> WorkflowError {
    WorkflowError::Store(err.to_string())
}
