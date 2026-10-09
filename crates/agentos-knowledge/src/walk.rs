//! Repository walk.
//!
//! Honors each project's `.gitignore` and `.agentosignore`. Parent and global gitignore
//! files are ignored so the same tree always yields the same file list. `.git` and
//! `.agentos` are always skipped. Whitespace-only files and files that contain a NUL
//! in the first 8 KiB are not indexed.

use std::fs::{self, DirEntry};
use std::path::{Path, PathBuf};

use crate::error::KnowledgeError;
use crate::hashutil::sha256_hex;

const BINARY_SNIFF: usize = 8192;

/// One text file that the scanner will consider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFile {
    /// Path relative to the project root, `/`-separated, no leading `./`.
    pub relative: String,
    /// Raw bytes.
    pub bytes: Vec<u8>,
    /// SHA-256 of `bytes`.
    pub fingerprint: String,
}

/// Lists indexable files in deterministic order.
pub fn list_sources(root: &Path) -> Result<Vec<SourceFile>, KnowledgeError> {
    let mut out = Vec::new();
    walk(root, "", &[], &mut out)?;
    out.sort_by(|a, b| a.relative.cmp(&b.relative));
    Ok(out)
}

fn walk(
    dir: &Path,
    rel: &str,
    inherited: &[Pattern],
    out: &mut Vec<SourceFile>,
) -> Result<(), KnowledgeError> {
    let mut patterns = inherited.to_vec();
    patterns.extend(load_ignore_files(dir, rel)?);
    let mut entries: Vec<DirEntry> = fs::read_dir(dir)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if name == ".git" || name == ".agentos" {
            continue;
        }
        let child_rel = if rel.is_empty() {
            name.to_owned()
        } else {
            format!("{rel}/{name}")
        };
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            continue;
        }
        let is_dir = file_type.is_dir();
        if is_ignored(&patterns, &child_rel, is_dir) {
            continue;
        }
        if is_dir {
            walk(&entry.path(), &child_rel, &patterns, out)?;
        } else if file_type.is_file() {
            consider(entry.path(), child_rel, out)?;
        }
    }
    Ok(())
}

fn consider(
    path: PathBuf,
    relative: String,
    out: &mut Vec<SourceFile>,
) -> Result<(), KnowledgeError> {
    let bytes = fs::read(&path)?;
    if bytes.is_empty() || bytes.iter().take(BINARY_SNIFF).any(|b| *b == 0) {
        return Ok(());
    }
    let Ok(text) = std::str::from_utf8(&bytes) else {
        return Ok(());
    };
    if text.trim().is_empty() {
        return Ok(());
    }
    let fingerprint = sha256_hex(&bytes);
    out.push(SourceFile {
        relative,
        bytes,
        fingerprint,
    });
    Ok(())
}

#[derive(Debug, Clone)]
struct Pattern {
    base: String,
    negation: bool,
    dir_only: bool,
    anchored: bool,
    glob: String,
}

fn load_ignore_files(dir: &Path, base: &str) -> Result<Vec<Pattern>, KnowledgeError> {
    let mut patterns = Vec::new();
    for name in [".gitignore", ".agentosignore"] {
        let path = dir.join(name);
        if !path.is_file() {
            continue;
        }
        let text = fs::read_to_string(&path)?;
        for line in text.lines() {
            if let Some(pattern) = parse_pattern(base, line) {
                patterns.push(pattern);
            }
        }
    }
    Ok(patterns)
}

fn parse_pattern(base: &str, raw: &str) -> Option<Pattern> {
    let line = trim_unescaped_trailing_space(raw);
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let negation = line.starts_with('!');
    let line = line.strip_prefix('!').unwrap_or(line);
    if line.is_empty() {
        return None;
    }
    let dir_only = line.ends_with('/');
    let line = line.trim_end_matches('/');
    let anchored = line.starts_with('/') || line.contains('/');
    let glob = line.trim_start_matches('/').to_owned();
    if glob.is_empty() {
        return None;
    }
    Some(Pattern {
        base: base.to_owned(),
        negation,
        dir_only,
        anchored,
        glob,
    })
}

fn trim_unescaped_trailing_space(line: &str) -> &str {
    let mut end = line.len();
    let bytes = line.as_bytes();
    while end > 0 && bytes[end - 1] == b' ' {
        let escaped = end >= 2 && bytes[end - 2] == b'\\';
        if escaped {
            break;
        }
        end -= 1;
    }
    &line[..end]
}

fn is_ignored(patterns: &[Pattern], rel: &str, is_dir: bool) -> bool {
    let mut ignored = false;
    for pattern in patterns {
        if pattern_matches(pattern, rel, is_dir) {
            ignored = !pattern.negation;
        }
    }
    ignored
}

fn pattern_matches(pattern: &Pattern, rel: &str, is_dir: bool) -> bool {
    if pattern.dir_only && !is_dir {
        return false;
    }
    let Some(rest) = strip_base(&pattern.base, rel) else {
        return false;
    };
    if rest.is_empty() {
        return false;
    }
    if pattern.anchored {
        glob_match(&pattern.glob, rest)
    } else {
        suffixes(rest)
            .iter()
            .any(|suffix| glob_match(&pattern.glob, suffix))
    }
}

fn strip_base<'a>(base: &str, rel: &'a str) -> Option<&'a str> {
    if base.is_empty() {
        return Some(rel);
    }
    rel.strip_prefix(base)?.strip_prefix('/')
}

fn suffixes(path: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    loop {
        out.push(&path[start..]);
        match path[start..].find('/') {
            Some(index) => start += index + 1,
            None => break,
        }
    }
    out
}

fn glob_match(pattern: &str, path: &str) -> bool {
    let pat: Vec<&str> = pattern.split('/').filter(|s| !s.is_empty()).collect();
    let text: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    glob_rec(&pat, &text)
}

fn glob_rec(pattern: &[&str], text: &[&str]) -> bool {
    match pattern.split_first() {
        None => text.is_empty(),
        Some((&"**", rest)) => {
            if glob_rec(rest, text) {
                return true;
            }
            if text.is_empty() {
                return false;
            }
            glob_rec(pattern, &text[1..])
        }
        Some((segment, rest)) => match text.split_first() {
            Some((head, tail)) if segment_match(segment, head) => glob_rec(rest, tail),
            _ => false,
        },
    }
}

fn segment_match(pattern: &str, text: &str) -> bool {
    fn rec(pattern: &[u8], text: &[u8]) -> bool {
        match (pattern.first().copied(), text.first().copied()) {
            (None, None) => true,
            (Some(b'*'), _) => {
                rec(&pattern[1..], text) || (!text.is_empty() && rec(pattern, &text[1..]))
            }
            (Some(b'?'), Some(_)) => rec(&pattern[1..], &text[1..]),
            (Some(a), Some(b)) if a == b => rec(&pattern[1..], &text[1..]),
            _ => false,
        }
    }
    rec(pattern.as_bytes(), text.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, rel: &str, body: &str) {
        let path = dir.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, body).unwrap();
    }

    #[test]
    fn gitignore_and_agentosignore_hide_files() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write(root, ".gitignore", "*.pyc\nbuild/\n# comment\n");
        write(root, ".agentosignore", "expected/\n");
        write(root, "keep.py", "x = 1\n");
        write(root, "skip.pyc", "nope\n");
        write(root, "build/hidden.py", "x = 1\n");
        write(root, "expected/graph.json", "{}\n");
        write(root, "pkg/nested.pyc", "nope\n");
        let files = list_sources(root).unwrap();
        let names: Vec<&str> = files.iter().map(|f| f.relative.as_str()).collect();
        assert_eq!(names, vec![".agentosignore", ".gitignore", "keep.py"]);
    }

    #[test]
    fn negation_keeps_a_file() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write(root, ".gitignore", "*.py\n!keep.py\n");
        write(root, "keep.py", "x = 1\n");
        write(root, "drop.py", "x = 1\n");
        let names: Vec<String> = list_sources(root)
            .unwrap()
            .into_iter()
            .map(|f| f.relative)
            .collect();
        assert!(names.contains(&"keep.py".to_owned()));
        assert!(!names.contains(&"drop.py".to_owned()));
    }

    #[test]
    fn derived_dirs_and_empty_files_are_skipped() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write(root, ".git/HEAD", "ref: refs/heads/main\n");
        write(root, ".agentos/config.toml", "x = 1\n");
        write(root, "empty.py", "   \n");
        write(root, "real.py", "x = 1\n");
        let names: Vec<String> = list_sources(root)
            .unwrap()
            .into_iter()
            .map(|f| f.relative)
            .collect();
        assert_eq!(names, vec!["real.py".to_owned()]);
    }
}
