//! Shared parsing helpers. Each provider keeps its own output grammar.

use crate::error::FeedbackError;
use crate::provider::split_command;
use crate::result::{Diagnostic, one_line};

/// A location a tool printed as `path:line` plus the rest of the line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Located {
    /// File path as printed.
    pub file: String,
    /// 1-based line.
    pub line: u32,
    /// Remainder of the line, already trimmed.
    pub message: String,
}

/// Finds `name.ext:line` in `line` for one of `extensions` (without the dot).
pub fn find_location(line: &str, extensions: &[&str]) -> Option<Located> {
    let bytes = line.as_bytes();
    let mut search_from = 0;
    while search_from < line.len() {
        let rest = &line[search_from..];
        let colon = rest.find(':')?;
        let abs = search_from + colon;
        let start = line[..abs]
            .rfind(char::is_whitespace)
            .map(|i| i + 1)
            .unwrap_or(0);
        let token = &line[start..abs];
        if extensions
            .iter()
            .any(|ext| token.ends_with(&format!(".{ext}")))
            && let Some(located) = read_at(line, start, abs, bytes)
        {
            return Some(located);
        }
        search_from = abs + 1;
    }
    None
}

fn read_at(line: &str, start: usize, colon: usize, bytes: &[u8]) -> Option<Located> {
    let mut index = colon + 1;
    if index >= bytes.len() || !bytes[index].is_ascii_digit() {
        return None;
    }
    let mut line_no: u32 = 0;
    while index < bytes.len() && bytes[index].is_ascii_digit() {
        line_no = line_no
            .saturating_mul(10)
            .saturating_add(u32::from(bytes[index] - b'0'));
        index += 1;
    }
    if line_no == 0 {
        return None;
    }
    let raw = line[start..colon].trim().trim_matches('`');
    let file = raw
        .rsplit(['(', '`'])
        .next()
        .unwrap_or(raw)
        .trim()
        .trim_matches('(')
        .to_owned();
    if file.is_empty() || file.contains(' ') {
        return None;
    }
    let message = line[index..]
        .trim()
        .trim_start_matches(':')
        .trim()
        .to_owned();
    Some(Located {
        file,
        line: line_no,
        message,
    })
}

/// Pushes a diagnostic when the same file, line, and summary are not already present.
pub fn push_diagnostic(
    out: &mut Vec<Diagnostic>,
    tool: &str,
    file: &str,
    line: u32,
    message: &str,
) {
    let summary = {
        let text = if message.is_empty() {
            format!("{file}:{line}")
        } else {
            message.to_owned()
        };
        one_line(&text)
    };
    if summary.is_empty() {
        return;
    }
    let diagnostic = Diagnostic {
        tool: tool.to_owned(),
        summary,
        file: Some(file.to_owned()),
        line: Some(line),
    };
    if !out.contains(&diagnostic) {
        out.push(diagnostic);
    }
}

/// Uses the override argv when present.
pub fn argv(default: &[&str], override_cmd: Option<&str>) -> Result<Vec<String>, FeedbackError> {
    if let Some(command) = override_cmd {
        return split_command(command);
    }
    Ok(default.iter().map(|part| (*part).to_owned()).collect())
}

/// True when `root/name` is a file.
pub fn has_file(root: &std::path::Path, name: &str) -> bool {
    root.join(name).is_file()
}

/// True when `dir` contains a file whose name matches `pred`, one level deep.
pub fn dir_has(root: &std::path::Path, dir: &str, pred: impl Fn(&str) -> bool) -> bool {
    let path = root.join(dir);
    let Ok(entries) = std::fs::read_dir(&path) else {
        return false;
    };
    entries.flatten().any(|entry| {
        entry
            .file_name()
            .to_str()
            .is_some_and(|name| pred(name) && entry.path().is_file())
    })
}
