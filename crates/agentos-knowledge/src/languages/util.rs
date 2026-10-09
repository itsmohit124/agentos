//! Tree-sitter helpers shared by language extractors.

use std::collections::BTreeMap;

use tree_sitter::Node;

use crate::extract::RawCall;

pub fn text<'a>(node: Node<'_>, source: &'a [u8]) -> &'a str {
    node.utf8_text(source).unwrap_or("")
}

pub fn lines(node: Node<'_>) -> (u32, u32) {
    let start = u32::try_from(node.start_position().row + 1).unwrap_or(u32::MAX);
    let mut end = u32::try_from(node.end_position().row + 1).unwrap_or(u32::MAX);
    if node.end_position().column == 0 && end > start {
        end -= 1;
    }
    (start, end.max(start))
}

pub fn named<'t>(node: Node<'t>) -> Vec<Node<'t>> {
    (0..node.named_child_count())
        .filter_map(|index| {
            let index = u32::try_from(index).ok()?;
            node.named_child(index)
        })
        .collect()
}

pub fn field<'t>(node: Node<'t>, name: &str) -> Option<Node<'t>> {
    node.child_by_field_name(name)
}

pub fn summarize(body_text: &str, fallback: &str) -> String {
    let chosen = if body_text.trim().is_empty() {
        fallback
    } else {
        body_text
    };
    let line = chosen.lines().next().unwrap_or("").trim();
    let line = line.trim_matches(|c| c == '"' || c == '\'' || c == '`');
    let mut out: String = line.chars().take(160).collect();
    if line.chars().count() > 160 {
        out.push('…');
    }
    out
}

/// Sorts calls so identical sites collapse.
pub fn dedup_calls(calls: &mut Vec<RawCall>) {
    calls.sort_by(|a, b| (&a.line, &a.callee, &a.receiver).cmp(&(&b.line, &b.callee, &b.receiver)));
    calls.dedup();
}

/// Dotted module path for an import specifier.
///
/// Relative specifiers (`./`, `../`, or `force_relative`) resolve against the importing file.
/// A leading `src/` or `lib/` is stripped once, matching [`crate::id::module_path`].
/// A known source extension is removed so `./service.ts` and `./service` agree.
pub fn import_module(from_file: &str, spec: &str, force_relative: bool) -> String {
    let spec = spec
        .trim()
        .trim_matches(|c| c == '"' || c == '\'' || c == '`');
    let spec = spec.split(['?', '#']).next().unwrap_or(spec).trim();
    if spec.is_empty() {
        return String::new();
    }
    let relative = force_relative || spec.starts_with("./") || spec.starts_with("../");
    let joined = if relative {
        let parent = from_file.rsplit_once('/').map(|(dir, _)| dir).unwrap_or("");
        if parent.is_empty() {
            spec.to_owned()
        } else {
            format!("{parent}/{spec}")
        }
    } else {
        spec.trim_start_matches('/').to_owned()
    };
    let mut parts = Vec::new();
    for part in joined.split(['/', '\\']) {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            parts.pop();
        } else {
            parts.push(part.to_owned());
        }
    }
    if let Some(last) = parts.last_mut()
        && let Some(stripped) = strip_source_ext(last)
    {
        *last = stripped;
    }
    if matches!(parts.first().map(String::as_str), Some("src" | "lib")) {
        parts.remove(0);
    }
    parts.join(".")
}

/// Last path segment of a type: `pkg.Payment`, `*Payment`, `Payment<T>` → `Payment`.
pub fn type_name(raw: &str) -> String {
    let raw = raw.trim().trim_start_matches(':').trim();
    let raw = raw.trim_start_matches(['*', '&']);
    let raw = raw.split(['<', '[']).next().unwrap_or(raw).trim();
    let raw = raw.trim_end_matches(['*', '&', '?', ' ']);
    raw.rsplit(['.', ':'])
        .next()
        .unwrap_or(raw)
        .trim()
        .trim_matches(|c| c == '*' || c == '&')
        .to_owned()
}

/// Comment immediately above `start_byte` (block comment or a run of `#` / `//` lines).
pub fn preceding_comment(source: &str, start_byte: usize) -> String {
    let head = &source[..start_byte.min(source.len())];
    let trimmed = head.trim_end_matches([' ', '\t', '\r']);
    let trimmed = trimmed.trim_end_matches('\n').trim_end_matches([' ', '\t']);
    if trimmed.ends_with("*/")
        && let Some(start) = trimmed.rfind("/*")
    {
        return trimmed[start..].to_owned();
    }
    let mut lines = Vec::new();
    for line in trimmed.lines().rev() {
        let text = line.trim();
        if text.starts_with('#') || text.starts_with("//") {
            lines.push(line);
        } else if text.is_empty() && lines.is_empty() {
            continue;
        } else {
            break;
        }
    }
    lines.reverse();
    lines.join("\n")
}

/// Declared parameter types from JSDoc `@param {Type} name` or YARD `@param name [Type]`.
pub fn comment_param_types(comment: &str) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    for line in comment.lines() {
        let line = line
            .trim()
            .trim_start_matches("///")
            .trim_start_matches("//")
            .trim_start_matches('#')
            .trim()
            .trim_start_matches('*')
            .trim();
        let Some(rest) = line.strip_prefix("@param") else {
            continue;
        };
        let rest = rest.trim();
        if let Some((name, ty)) = jsdoc_param(rest).or_else(|| yard_param(rest)) {
            let ty = type_name(&ty);
            if !name.is_empty() && !ty.is_empty() && !name.contains('.') {
                map.insert(name, ty);
            }
        }
    }
    map
}

/// Walks up a few parents so a comment above `const f = () =>` is visible from the arrow.
pub fn attached_comment(node: Node<'_>, source: &str) -> String {
    let mut current = Some(node);
    for _ in 0..6 {
        let Some(cursor) = current else {
            break;
        };
        let comment = preceding_comment(source, cursor.start_byte());
        if comment.contains("@param") {
            return comment;
        }
        current = cursor.parent();
    }
    String::new()
}

fn jsdoc_param(rest: &str) -> Option<(String, String)> {
    let rest = rest.strip_prefix('{')?;
    let (ty, rest) = rest.split_once('}')?;
    let name = rest
        .trim()
        .trim_start_matches('[')
        .split(|c: char| c.is_whitespace() || c == ']' || c == '=' || c == '-')
        .next()
        .unwrap_or("")
        .trim();
    if name.is_empty() {
        return None;
    }
    Some((name.to_owned(), ty.to_owned()))
}

fn yard_param(rest: &str) -> Option<(String, String)> {
    if let Some(rest) = rest.strip_prefix('[') {
        let (ty, rest) = rest.split_once(']')?;
        let name = rest.split_whitespace().next().unwrap_or("").trim();
        if name.is_empty() {
            return None;
        }
        return Some((name.to_owned(), ty.to_owned()));
    }
    let mut parts = rest.split_whitespace();
    let name = parts.next()?.trim();
    let bracket = parts.next()?;
    let ty = bracket.strip_prefix('[')?.strip_suffix(']')?.trim();
    if name.is_empty() || name.starts_with('[') {
        return None;
    }
    Some((name.to_owned(), ty.to_owned()))
}

fn strip_source_ext(name: &str) -> Option<String> {
    const EXTS: &[&str] = &[
        ".tsx", ".ts", ".jsx", ".js", ".mjs", ".cjs", ".go", ".java", ".rb", ".py",
    ];
    EXTS.iter()
        .find_map(|ext| name.strip_suffix(ext).map(str::to_owned))
}

pub fn unquote_doc(raw: &str) -> String {
    let trimmed = raw.trim();
    for quote in ["\"\"\"", "'''"] {
        if let Some(inner) = trimmed
            .strip_prefix(quote)
            .and_then(|s| s.strip_suffix(quote))
        {
            return inner.trim().to_owned();
        }
    }
    trimmed
        .trim_matches(|c| c == '"' || c == '\'')
        .trim()
        .to_owned()
}
