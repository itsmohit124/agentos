//! File cache of successful packs.
//!
//! The key is the knowledge fingerprint, the task, the budget, and the ranking configuration.
//! Deleting the directory is safe: the next build writes it again.

use std::fs;
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::error::ContextError;
use crate::memory::{self, MemoryCandidate};
use crate::view::hex_encode;

/// Ranking configuration mixed into the cache key. Bump when selection rules change.
pub const CONFIG_ID: &str = "context-v1";

/// Cache key. The same inputs always produce the same key.
pub fn cache_key(fingerprint: &str, task: &str, budget: Option<u32>) -> String {
    let budget = budget
        .map(|value| value.to_string())
        .unwrap_or_else(|| "none".to_owned());
    let mut hasher = Sha256::new();
    hasher.update(fingerprint.as_bytes());
    hasher.update(b"\n");
    hasher.update(task.as_bytes());
    hasher.update(b"\n");
    hasher.update(budget.as_bytes());
    hasher.update(b"\n");
    hasher.update(CONFIG_ID.as_bytes());
    hex_encode(&hasher.finalize())
}

/// Cache key when memory candidates are present. Empty memory keeps [`cache_key`].
pub fn cache_key_with_memory(
    fingerprint: &str,
    task: &str,
    budget: Option<u32>,
    memory: &[MemoryCandidate],
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(cache_key(fingerprint, task, budget).as_bytes());
    hasher.update(b"\ncontext-v1-memory\n");
    hasher.update(memory::memory_fingerprint(memory).as_bytes());
    hex_encode(&hasher.finalize())
}

/// Reads a cached pack. Missing files return `None`.
pub fn read(dir: &Path, key: &str) -> Result<Option<String>, ContextError> {
    let path = dir.join(format!("{key}.json"));
    if !path.is_file() {
        return Ok(None);
    }
    let text = fs::read_to_string(&path)?;
    if text.is_empty() {
        return Ok(None);
    }
    Ok(Some(text))
}

/// Writes a pack. A crash mid-write leaves the previous file in place.
pub fn write(dir: &Path, key: &str, body: &str) -> Result<(), ContextError> {
    fs::create_dir_all(dir)?;
    let path = dir.join(format!("{key}.json"));
    let temporary = dir.join(format!("{key}.json.tmp"));
    fs::write(&temporary, body)?;
    fs::rename(&temporary, &path)?;
    Ok(())
}
