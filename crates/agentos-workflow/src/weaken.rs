//! Detection of test deletion and assertion weakening during a fix.
//!
//! The existing test text is authoritative. A fix that deletes a test file, removes a
//! test, or lowers the assertion count fails the run. Production files are ignored.

use serde::{Deserialize, Serialize};

/// One before/after pair reported by a runner.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TestEdit {
    /// Project-relative path.
    pub path: String,
    /// File text before the fix. Empty when the file is new.
    pub before: String,
    /// File text after the fix. Empty when the file was deleted.
    pub after: String,
}

/// Why a fix was rejected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WeakenFinding {
    /// Project-relative path.
    pub path: String,
    /// `deleted`, `removed_test`, or `assertion`.
    pub kind: String,
    /// One deterministic sentence.
    pub detail: String,
}

/// Returns the first weakening in `edits`, in path order.
pub fn detect(edits: &[TestEdit]) -> Option<WeakenFinding> {
    let mut edits = edits.to_vec();
    edits.sort_by(|left, right| left.path.cmp(&right.path));
    for edit in edits {
        if !is_test_path(&edit.path) {
            continue;
        }
        if !edit.before.is_empty() && edit.after.is_empty() {
            return Some(WeakenFinding {
                path: edit.path.clone(),
                kind: "deleted".to_owned(),
                detail: format!("deleted {}", edit.path),
            });
        }
        let removed = removed_tests(&edit.before, &edit.after);
        if let Some(name) = removed.first() {
            return Some(WeakenFinding {
                path: edit.path.clone(),
                kind: "removed_test".to_owned(),
                detail: format!("removed test {name} from {}", edit.path),
            });
        }
        let before_count = assertion_count(&edit.before);
        let after_count = assertion_count(&edit.after);
        if after_count < before_count {
            return Some(WeakenFinding {
                path: edit.path.clone(),
                kind: "assertion".to_owned(),
                detail: format!(
                    "weakened assertion in {}: {before_count} -> {after_count}",
                    edit.path
                ),
            });
        }
    }
    None
}

fn is_test_path(path: &str) -> bool {
    let path = path.replace('\\', "/");
    let name = path.rsplit('/').next().unwrap_or(path.as_str());
    path.starts_with("tests/")
        || path.contains("/tests/")
        || name.starts_with("test_")
        || name.ends_with("_test.rs")
        || name.ends_with("_test.go")
        || name.ends_with("_test.py")
        || name.ends_with(".test.js")
        || name.ends_with(".test.ts")
        || name.ends_with(".test.tsx")
        || name.ends_with(".spec.js")
        || name.ends_with(".spec.ts")
        || name.ends_with(".spec.rb")
        || name.ends_with("_spec.rb")
}

fn assertion_count(text: &str) -> usize {
    const PATTERNS: &[&str] = &[
        "assert_eq",
        "assert_ne",
        "assert!",
        "assert ",
        "assert(",
        "expect(",
        "pytest.raises",
        ".toBe(",
        ".toEqual(",
    ];
    PATTERNS
        .iter()
        .map(|pattern| text.matches(pattern).count())
        .sum()
}

fn removed_tests(before: &str, after: &str) -> Vec<String> {
    let after_names = test_names(after);
    let mut removed: Vec<String> = test_names(before)
        .into_iter()
        .filter(|name| !after_names.contains(name))
        .collect();
    removed.sort();
    removed.dedup();
    removed
}

fn test_names(text: &str) -> Vec<String> {
    let mut names = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(name) = rust_or_python(trimmed) {
            names.push(name);
        }
        if let Some(name) = quoted_name(trimmed, "it(").or_else(|| quoted_name(trimmed, "test(")) {
            names.push(name);
        }
    }
    names
}

fn rust_or_python(line: &str) -> Option<String> {
    for prefix in ["fn ", "def ", "async fn ", "async def "] {
        let Some(rest) = line.strip_prefix(prefix) else {
            continue;
        };
        let name: String = rest
            .chars()
            .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
            .collect();
        if name.starts_with("test_") || (name.starts_with("test") && name.len() > 4) {
            return Some(name);
        }
    }
    None
}

fn quoted_name(line: &str, prefix: &str) -> Option<String> {
    let rest = line.strip_prefix(prefix)?.trim_start();
    let quote = rest.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let body = rest[1..].split(quote).next().unwrap_or("");
    if body.is_empty() {
        None
    } else {
        Some(body.to_owned())
    }
}
