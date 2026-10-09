//! Markdown documents and architecture decision records.
//!
//! Documents are declared knowledge: a person wrote them. Mentions of code symbols are
//! resolved later and recorded as inferred, never promoted.

use crate::id::{self, document_id};
use crate::model::NodeKind;

/// One markdown file turned into a document or decision plus its sections.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocExtract {
    /// [`NodeKind::Document`] or [`NodeKind::Decision`].
    pub kind: NodeKind,
    /// Logical id.
    pub id: String,
    /// File stem.
    pub name: String,
    /// First heading, or the file stem.
    pub summary: String,
    /// Sections in source order.
    pub sections: Vec<Section>,
}

/// One heading section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    /// 1-based start line.
    pub start_line: u32,
    /// 1-based end line.
    pub end_line: u32,
    /// Section text, including the heading.
    pub content: String,
}

/// Parses a markdown file. ADRs (a path component `adr`) become decisions.
pub fn extract(relative: &str, source: &str) -> DocExtract {
    let kind = if id::is_adr(relative) {
        NodeKind::Decision
    } else {
        NodeKind::Document
    };
    let id_kind = if kind == NodeKind::Decision {
        "decision"
    } else {
        "document"
    };
    let name = relative
        .rsplit('/')
        .next()
        .unwrap_or(relative)
        .rsplit_once('.')
        .map(|(stem, _)| stem)
        .unwrap_or(relative)
        .to_owned();
    let sections = sections(source);
    let summary = sections
        .first()
        .and_then(|section| heading_text(&section.content))
        .unwrap_or_else(|| name.clone());
    DocExtract {
        kind,
        id: document_id(id_kind, relative),
        name,
        summary,
        sections,
    }
}

fn sections(source: &str) -> Vec<Section> {
    let lines: Vec<&str> = source.split('\n').collect();
    let mut starts = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        if line.starts_with('#') {
            starts.push(index);
        }
    }
    if starts.is_empty() {
        return vec![Section {
            start_line: 1,
            end_line: u32::try_from(lines.len().max(1)).unwrap_or(u32::MAX),
            content: source.to_owned(),
        }];
    }
    let mut out = Vec::new();
    for (pos, start) in starts.iter().copied().enumerate() {
        let end = starts.get(pos + 1).copied().unwrap_or(lines.len());
        let content = lines[start..end].join("\n");
        if content.trim().is_empty() {
            continue;
        }
        out.push(Section {
            start_line: u32::try_from(start + 1).unwrap_or(u32::MAX),
            end_line: u32::try_from(end.max(start + 1)).unwrap_or(u32::MAX),
            content,
        });
    }
    out
}

fn heading_text(content: &str) -> Option<String> {
    let line = content.lines().next()?.trim();
    let text = line.trim_start_matches('#').trim();
    if text.is_empty() {
        None
    } else {
        Some(text.to_owned())
    }
}

/// Whole-word ASCII match. `Payment` does not match inside `PaymentService`.
pub fn contains_word(haystack: &str, word: &str) -> bool {
    if word.is_empty() || word.len() < 4 {
        return false;
    }
    let bytes = haystack.as_bytes();
    let needle = word.as_bytes();
    let mut start = 0;
    while let Some(offset) = haystack[start..].find(word) {
        let index = start + offset;
        let before_ok = index == 0 || !is_ident(bytes[index - 1]);
        let after = index + needle.len();
        let after_ok = after >= bytes.len() || !is_ident(bytes[after]);
        if before_ok && after_ok {
            return true;
        }
        start = index + 1;
    }
    false
}

fn is_ident(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// 1-based line of the first whole-word hit, if any.
pub fn word_line(content: &str, word: &str) -> Option<u32> {
    for (index, line) in content.lines().enumerate() {
        if contains_word(line, word) {
            return u32::try_from(index + 1).ok();
        }
    }
    None
}
