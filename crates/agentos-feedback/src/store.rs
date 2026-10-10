//! Derived validation history under `.agentos/feedback/`.
//!
//! The index order is the run order. `validate last` is the last id in that index, not a
//! timestamp. Raw output is stored once, next to the result, and the result keeps only the path.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::FeedbackError;
use crate::result::ValidationResult;

const RAW_SPLIT: &str = "\n--- stderr ---\n";

#[derive(Debug, Default, Serialize, Deserialize)]
struct IndexFile {
    ids: Vec<String>,
}

/// Appends `result` and its capped output. Returns the stored result, including its id.
pub fn store_result(
    root: &Path,
    result: &ValidationResult,
    stdout: &str,
    stderr: &str,
) -> Result<ValidationResult, FeedbackError> {
    let dir = feedback_dir(root);
    fs::create_dir_all(dir.join("results"))?;
    fs::create_dir_all(dir.join("raw"))?;
    let mut index = read_index(&dir)?;
    let id = format!(
        "{}-{}-{}",
        result.provider,
        result.kind.as_str(),
        index.ids.len() + 1
    );
    if !valid_id(&id) {
        return Err(FeedbackError::InvalidId(id));
    }
    let mut stored = result.clone();
    stored.assign_id(id.clone());
    let raw_rel = format!(".agentos/feedback/raw/{id}.log");
    stored.raw_output_ref = Some(raw_rel.clone());
    if !stored.artifacts.contains(&raw_rel) {
        stored.artifacts.push(raw_rel);
        stored.artifacts.sort();
    }
    fs::write(
        dir.join("raw").join(format!("{id}.log")),
        format!("{stdout}{RAW_SPLIT}{stderr}"),
    )?;
    let json = serde_json::to_string_pretty(&stored)
        .map_err(|err| FeedbackError::Config(err.to_string()))?;
    fs::write(
        dir.join("results").join(format!("{id}.json")),
        format!("{json}\n"),
    )?;
    index.ids.push(id);
    write_index(&dir, &index)?;
    Ok(stored)
}

/// The most recently stored result.
pub fn last_result(root: &Path) -> Result<ValidationResult, FeedbackError> {
    let dir = feedback_dir(root);
    let index = read_index(&dir)?;
    let Some(id) = index.ids.last() else {
        return Err(FeedbackError::NoResult);
    };
    load_result(root, id)
}

/// Loads one stored result by id.
pub fn load_result(root: &Path, id: &str) -> Result<ValidationResult, FeedbackError> {
    if !valid_id(id) {
        return Err(FeedbackError::InvalidId(id.to_owned()));
    }
    let path = feedback_dir(root)
        .join("results")
        .join(format!("{id}.json"));
    if !path.is_file() {
        return Err(FeedbackError::UnknownResult(id.to_owned()));
    }
    let text = fs::read_to_string(path)?;
    serde_json::from_str(&text).map_err(|err| FeedbackError::Config(err.to_string()))
}

/// Reads the capped stdout and stderr stored for `id`.
pub fn load_raw(root: &Path, id: &str) -> Result<(String, String), FeedbackError> {
    if !valid_id(id) {
        return Err(FeedbackError::InvalidId(id.to_owned()));
    }
    let path = feedback_dir(root).join("raw").join(format!("{id}.log"));
    let text = fs::read_to_string(path)?;
    Ok(split_raw(&text))
}

/// Splits a stored raw log into stdout and stderr.
pub fn split_raw(text: &str) -> (String, String) {
    match text.split_once(RAW_SPLIT) {
        Some((stdout, stderr)) => (stdout.to_owned(), stderr.to_owned()),
        None => (text.to_owned(), String::new()),
    }
}

fn feedback_dir(root: &Path) -> PathBuf {
    root.join(".agentos/feedback")
}

fn read_index(dir: &Path) -> Result<IndexFile, FeedbackError> {
    let path = dir.join("index.json");
    if !path.is_file() {
        return Ok(IndexFile::default());
    }
    let text = fs::read_to_string(path)?;
    serde_json::from_str(&text).map_err(|err| FeedbackError::Config(err.to_string()))
}

fn write_index(dir: &Path, index: &IndexFile) -> Result<(), FeedbackError> {
    let json = serde_json::to_string_pretty(index)
        .map_err(|err| FeedbackError::Config(err.to_string()))?;
    fs::write(dir.join("index.json"), format!("{json}\n"))?;
    Ok(())
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && !id.contains("..")
        && !id.contains('/')
        && !id.contains('\\')
        && id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.')
}
