//! TypeScript and JavaScript extractors (tree-sitter).
//!
//! Both languages share one walk. The extractor name follows the file extension so
//! provenance records which grammar ran.

use std::collections::BTreeMap;
use std::path::Path;

use tree_sitter::{Node, Parser};

use crate::error::KnowledgeError;
use crate::extract::{LanguageExtractor, RawCall, RawSymbol};
use crate::languages::util::{self, field, lines, named, text};
use crate::model::NodeKind;

/// TypeScript (`.ts`, `.tsx`).
#[derive(Debug, Default)]
pub struct TypeScriptExtractor;

/// JavaScript (`.js`, `.jsx`, `.mjs`, `.cjs`).
#[derive(Debug, Default)]
pub struct JavaScriptExtractor;

impl LanguageExtractor for TypeScriptExtractor {
    fn name(&self) -> &'static str {
        "typescript"
    }
    fn version(&self) -> &'static str {
        "1"
    }
    fn matches(&self, path: &Path) -> bool {
        matches!(
            path.extension().and_then(|ext| ext.to_str()),
            Some("ts" | "tsx")
        )
    }
    fn extract(&self, relative: &str, source: &str) -> Result<Vec<RawSymbol>, KnowledgeError> {
        let language = if relative.ends_with(".tsx") {
            tree_sitter_typescript::LANGUAGE_TSX
        } else {
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT
        };
        extract_ecma("typescript", language, relative, source)
    }
}

impl LanguageExtractor for JavaScriptExtractor {
    fn name(&self) -> &'static str {
        "javascript"
    }
    fn version(&self) -> &'static str {
        "1"
    }
    fn matches(&self, path: &Path) -> bool {
        matches!(
            path.extension().and_then(|ext| ext.to_str()),
            Some("js" | "jsx" | "mjs" | "cjs")
        )
    }
    fn extract(&self, relative: &str, source: &str) -> Result<Vec<RawSymbol>, KnowledgeError> {
        extract_ecma(
            "javascript",
            tree_sitter_javascript::LANGUAGE,
            relative,
            source,
        )
    }
}

fn extract_ecma(
    language_name: &'static str,
    language: impl Into<tree_sitter::Language>,
    relative: &str,
    source: &str,
) -> Result<Vec<RawSymbol>, KnowledgeError> {
    let mut parser = Parser::new();
    let language = language.into();
    parser
        .set_language(&language)
        .map_err(|err| KnowledgeError::Extract {
            language: language_name,
            message: err.to_string(),
        })?;
    let tree = parser
        .parse(source, None)
        .ok_or_else(|| KnowledgeError::Extract {
            language: language_name,
            message: format!("no tree for {relative}"),
        })?;
    let root = tree.root_node();
    if root.has_error() {
        tracing::warn!(
            file = relative,
            language = language_name,
            "parse error; extracting what parsed"
        );
    }
    let bytes = source.as_bytes();
    let mut symbols = Vec::new();
    for child in named(root) {
        push_item(child, bytes, &mut symbols);
    }
    Ok(symbols)
}

fn push_item(node: Node<'_>, source: &[u8], out: &mut Vec<RawSymbol>) {
    match node.kind() {
        "export_statement" => {
            if let Some(decl) = field(node, "declaration") {
                push_item(decl, source, out);
            }
        }
        "class_declaration" => push_class(node, source, out),
        "interface_declaration" => push_interface(node, source, out),
        "function_declaration" => push_function(node, source, None, NodeKind::Function, out),
        "type_alias_declaration" => push_alias(node, source, out),
        "lexical_declaration" => {
            for child in named(node) {
                if child.kind() == "variable_declarator" {
                    push_var(child, source, out);
                }
            }
        }
        "import_statement" => push_import(node, source, out),
        _ => {}
    }
}

fn push_class(node: Node<'_>, source: &[u8], out: &mut Vec<RawSymbol>) {
    let Some(name) = field(node, "name").map(|n| text(n, source).to_owned()) else {
        return;
    };
    if name.is_empty() {
        return;
    }
    let (start, end) = lines(node);
    let mut bases = Vec::new();
    let mut implements = Vec::new();
    for child in named(node) {
        if child.kind() != "class_heritage" {
            continue;
        }
        for part in named(child) {
            match part.kind() {
                "extends_clause" => {
                    let values: Vec<Node<'_>> = field(part, "value").into_iter().collect();
                    let values = if values.is_empty() {
                        named(part)
                    } else {
                        values
                    };
                    for value in values {
                        let base = simple_type(text(value, source));
                        if !base.is_empty() && base != "extends" {
                            bases.push(base);
                        }
                    }
                }
                "implements_clause" => {
                    for ty in named(part) {
                        let name = simple_type(text(ty, source));
                        if !name.is_empty() {
                            implements.push(name);
                        }
                    }
                }
                _ => {}
            }
        }
    }
    let body = text(node, source).to_owned();
    out.push(RawSymbol {
        kind: NodeKind::Class,
        name: name.clone(),
        parent: None,
        start_line: start,
        end_line: end,
        summary: util::summarize("", &body),
        body,
        bases,
        implements,
        calls: Vec::new(),
        import_target: None,
    });
    if let Some(body_node) = field(node, "body") {
        for child in named(body_node) {
            if child.kind() == "method_definition" {
                push_function(child, source, Some(&name), NodeKind::Method, out);
            }
        }
    }
}

fn push_interface(node: Node<'_>, source: &[u8], out: &mut Vec<RawSymbol>) {
    let Some(name) = field(node, "name").map(|n| text(n, source).to_owned()) else {
        return;
    };
    let (start, end) = lines(node);
    let body = text(node, source).to_owned();
    out.push(RawSymbol {
        kind: NodeKind::Interface,
        name: name.clone(),
        parent: None,
        start_line: start,
        end_line: end,
        summary: util::summarize("", &body),
        body,
        bases: Vec::new(),
        implements: Vec::new(),
        calls: Vec::new(),
        import_target: None,
    });
    if let Some(body_node) = field(node, "body") {
        for child in named(body_node) {
            if child.kind() == "method_signature" {
                push_function(child, source, Some(&name), NodeKind::Method, out);
            }
        }
    }
}

fn push_function(
    node: Node<'_>,
    source: &[u8],
    parent: Option<&str>,
    kind: NodeKind,
    out: &mut Vec<RawSymbol>,
) {
    let Some(name) = field(node, "name").map(|n| text(n, source).to_owned()) else {
        return;
    };
    if name.is_empty() {
        return;
    }
    let (start, end) = lines(node);
    let body_text = text(node, source).to_owned();
    let params = parameter_types(field(node, "parameters"), source);
    let calls = field(node, "body")
        .map(|body| collect_calls(body, source, &params))
        .unwrap_or_default();
    out.push(RawSymbol {
        kind,
        name,
        parent: parent.map(str::to_owned),
        start_line: start,
        end_line: end,
        summary: util::summarize("", &body_text),
        body: body_text,
        bases: Vec::new(),
        implements: Vec::new(),
        calls,
        import_target: None,
    });
}

fn push_alias(node: Node<'_>, source: &[u8], out: &mut Vec<RawSymbol>) {
    let Some(name) = field(node, "name").map(|n| text(n, source).to_owned()) else {
        return;
    };
    let (start, end) = lines(node);
    let body = text(node, source).to_owned();
    out.push(RawSymbol {
        kind: NodeKind::Symbol,
        name,
        parent: None,
        start_line: start,
        end_line: end,
        summary: util::summarize("", &body),
        body,
        bases: Vec::new(),
        implements: Vec::new(),
        calls: Vec::new(),
        import_target: None,
    });
}

fn push_var(node: Node<'_>, source: &[u8], out: &mut Vec<RawSymbol>) {
    let Some(name_node) = field(node, "name") else {
        return;
    };
    if name_node.kind() != "identifier" {
        return;
    }
    let name = text(name_node, source).to_owned();
    let (start, end) = lines(node);
    let body = text(node, source).to_owned();
    out.push(RawSymbol {
        kind: NodeKind::Symbol,
        name,
        parent: None,
        start_line: start,
        end_line: end,
        summary: util::summarize("", &body),
        body,
        bases: Vec::new(),
        implements: Vec::new(),
        calls: Vec::new(),
        import_target: None,
    });
}

fn push_import(node: Node<'_>, source: &[u8], out: &mut Vec<RawSymbol>) {
    let Some(source_node) = field(node, "source") else {
        return;
    };
    let raw = text(source_node, source)
        .trim()
        .trim_matches(|c| c == '"' || c == '\'' || c == '`');
    let target = raw
        .trim_start_matches("./")
        .trim_start_matches("../")
        .replace('/', ".");
    if target.is_empty() {
        return;
    }
    let (start, end) = lines(node);
    let body = text(node, source).to_owned();
    out.push(RawSymbol {
        kind: NodeKind::Import,
        name: target.clone(),
        parent: None,
        start_line: start,
        end_line: end,
        summary: util::summarize("", &body),
        body,
        bases: Vec::new(),
        implements: Vec::new(),
        calls: Vec::new(),
        import_target: Some(target),
    });
}

fn parameter_types(params: Option<Node<'_>>, source: &[u8]) -> BTreeMap<String, String> {
    let Some(params) = params else {
        return BTreeMap::new();
    };
    let mut map = BTreeMap::new();
    for child in named(params) {
        let Some(type_node) = field(child, "type").or_else(|| {
            named(child)
                .into_iter()
                .find(|n| n.kind() == "type_annotation")
        }) else {
            continue;
        };
        let type_name = simple_type(text(type_node, source));
        let name = field(child, "name")
            .or_else(|| field(child, "pattern"))
            .map(|n| text(n, source).to_owned());
        if let Some(name) = name.filter(|n| !n.is_empty() && !n.contains('.'))
            && !type_name.is_empty()
        {
            map.insert(name, type_name);
        }
    }
    map
}

fn collect_calls(node: Node<'_>, source: &[u8], params: &BTreeMap<String, String>) -> Vec<RawCall> {
    let mut out = Vec::new();
    walk_calls(node, source, params, &mut out);
    out.sort_by(|a, b| (&a.line, &a.callee, &a.receiver).cmp(&(&b.line, &b.callee, &b.receiver)));
    out.dedup();
    out
}

fn walk_calls(
    node: Node<'_>,
    source: &[u8],
    params: &BTreeMap<String, String>,
    out: &mut Vec<RawCall>,
) {
    if matches!(
        node.kind(),
        "function_declaration"
            | "class_declaration"
            | "method_definition"
            | "arrow_function"
            | "function"
    ) {
        return;
    }
    if node.kind() == "call_expression"
        && let Some(call) = read_call(node, source, params)
    {
        out.push(call);
    }
    for child in named(node) {
        walk_calls(child, source, params, out);
    }
}

fn read_call(node: Node<'_>, source: &[u8], params: &BTreeMap<String, String>) -> Option<RawCall> {
    let function = field(node, "function")?;
    let (callee, receiver) = match function.kind() {
        "identifier" => (text(function, source).to_owned(), None),
        "member_expression" => {
            let callee = field(function, "property").map(|n| text(n, source).to_owned())?;
            let receiver = field(function, "object").map(|n| text(n, source).to_owned());
            (callee, receiver)
        }
        _ => return None,
    };
    let receiver_type = receiver
        .as_deref()
        .and_then(|name| params.get(name))
        .cloned();
    let (line, _) = lines(node);
    Some(RawCall {
        callee,
        receiver,
        receiver_type,
        line,
    })
}

fn simple_type(raw: &str) -> String {
    let raw = raw.trim().trim_start_matches(':').trim();
    let raw = raw.split('<').next().unwrap_or(raw).trim();
    raw.rsplit('.').next().unwrap_or(raw).trim().to_owned()
}
