//! Tree-sitter helpers shared by language extractors.

use tree_sitter::Node;

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
