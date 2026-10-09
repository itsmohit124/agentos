//! TypeScript and JavaScript extractors (tree-sitter).
//!
//! Both languages share one walk. The extractor name follows the file extension so
//! provenance records which grammar ran.

use std::collections::BTreeMap;
use std::path::Path;

use tree_sitter::{Node, Parser};

use crate::error::KnowledgeError;
use crate::extract::{LanguageExtractor, RawCall, RawSymbol};
use crate::languages::util::{
    self, attached_comment, comment_param_types, dedup_calls, field, import_module, lines, named,
    text, type_name,
};
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
        "2"
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
        "2"
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
        push_item(child, relative, source, bytes, &mut symbols);
    }
    order_parents_first(&mut symbols);
    Ok(symbols)
}

fn order_parents_first(symbols: &mut [RawSymbol]) {
    symbols.sort_by(|a, b| {
        a.parent
            .is_some()
            .cmp(&b.parent.is_some())
            .then(a.start_line.cmp(&b.start_line))
            .then(a.name.cmp(&b.name))
            .then(a.kind.as_str().cmp(b.kind.as_str()))
    });
}

fn push_item(node: Node<'_>, relative: &str, full: &str, source: &[u8], out: &mut Vec<RawSymbol>) {
    match node.kind() {
        "export_statement" => {
            if let Some(spec) = field(node, "source") {
                push_import_spec(node, relative, text(spec, source), source, out);
            }
            if let Some(decl) = field(node, "declaration") {
                push_item(decl, relative, full, source, out);
            } else {
                for child in named(node) {
                    if is_declaration(child.kind()) {
                        push_item(child, relative, full, source, out);
                    }
                }
            }
        }
        "class_declaration" => push_class(node, full, source, out),
        "interface_declaration" => push_interface(node, full, source, out),
        "function_declaration" => {
            push_function(node, full, source, None, None, NodeKind::Function, out);
        }
        "type_alias_declaration" => push_alias(node, source, out),
        "lexical_declaration" | "variable_declaration" => {
            for child in named(node) {
                if child.kind() == "variable_declarator" {
                    push_var(child, relative, full, source, out);
                }
            }
        }
        "expression_statement" => {
            for child in named(node) {
                if let Some(spec) = require_specifier(child, source) {
                    push_import_spec(node, relative, &spec, source, out);
                }
            }
        }
        "import_statement" => push_import(node, relative, source, out),
        _ => {}
    }
}

fn is_declaration(kind: &str) -> bool {
    matches!(
        kind,
        "class_declaration"
            | "interface_declaration"
            | "function_declaration"
            | "generator_function_declaration"
            | "lexical_declaration"
            | "variable_declaration"
            | "type_alias_declaration"
    )
}

fn push_class(node: Node<'_>, full: &str, source: &[u8], out: &mut Vec<RawSymbol>) {
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
                        let base = type_name(text(value, source));
                        if !base.is_empty() && base != "extends" {
                            bases.push(base);
                        }
                    }
                }
                "implements_clause" => {
                    for ty in named(part) {
                        let name = type_name(text(ty, source));
                        if !name.is_empty() {
                            implements.push(name);
                        }
                    }
                }
                // JavaScript puts the superclass expression directly under class_heritage.
                "identifier" | "member_expression" | "nested_identifier" => {
                    let base = type_name(text(part, source));
                    if !base.is_empty() && base != "extends" {
                        bases.push(base);
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
            match child.kind() {
                "method_definition" => {
                    push_function(
                        child,
                        full,
                        source,
                        None,
                        Some(&name),
                        NodeKind::Method,
                        out,
                    );
                }
                "public_field_definition" => {
                    let Some(value) = field(child, "value").filter(|n| is_function_value(n.kind()))
                    else {
                        continue;
                    };
                    let field_name = field(child, "name").map(|n| text(n, source).to_owned());
                    if let Some(field_name) =
                        field_name.filter(|n| !n.is_empty() && !n.starts_with('#'))
                    {
                        push_function(
                            value,
                            full,
                            source,
                            Some(field_name),
                            Some(&name),
                            NodeKind::Method,
                            out,
                        );
                    }
                }
                _ => {}
            }
        }
    }
}

fn push_interface(node: Node<'_>, full: &str, source: &[u8], out: &mut Vec<RawSymbol>) {
    let Some(name) = field(node, "name").map(|n| text(n, source).to_owned()) else {
        return;
    };
    let (start, end) = lines(node);
    let mut bases = Vec::new();
    for child in named(node) {
        if child.kind() == "extends_type_clause" || child.kind() == "extends_clause" {
            for ty in named(child) {
                let base = type_name(text(ty, source));
                if !base.is_empty() && base != "extends" {
                    bases.push(base);
                }
            }
        }
    }
    let body = text(node, source).to_owned();
    out.push(RawSymbol {
        kind: NodeKind::Interface,
        name: name.clone(),
        parent: None,
        start_line: start,
        end_line: end,
        summary: util::summarize("", &body),
        body,
        bases,
        implements: Vec::new(),
        calls: Vec::new(),
        import_target: None,
    });
    if let Some(body_node) = field(node, "body") {
        for child in named(body_node) {
            if child.kind() == "method_signature" {
                push_function(
                    child,
                    full,
                    source,
                    None,
                    Some(&name),
                    NodeKind::Method,
                    out,
                );
            }
        }
    }
}

fn push_function(
    node: Node<'_>,
    full: &str,
    source: &[u8],
    name_override: Option<String>,
    parent: Option<&str>,
    kind: NodeKind,
    out: &mut Vec<RawSymbol>,
) {
    let name = name_override.or_else(|| field(node, "name").map(|n| text(n, source).to_owned()));
    let Some(name) = name.filter(|n| !n.is_empty()) else {
        return;
    };
    let (start, end) = lines(node);
    let body_text = text(node, source).to_owned();
    let mut params = parameter_types(function_params(node), source);
    for (param, ty) in comment_param_types(&attached_comment(node, full)) {
        params.entry(param).or_insert(ty);
    }
    let calls = call_root(node)
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

fn function_params(node: Node<'_>) -> Option<Node<'_>> {
    field(node, "parameters").or_else(|| field(node, "parameter"))
}

fn call_root(node: Node<'_>) -> Option<Node<'_>> {
    field(node, "body")
}

fn is_function_value(kind: &str) -> bool {
    matches!(
        kind,
        "arrow_function" | "function" | "function_expression" | "generator_function"
    )
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

fn push_var(node: Node<'_>, relative: &str, full: &str, source: &[u8], out: &mut Vec<RawSymbol>) {
    if let Some(value) = field(node, "value")
        && let Some(spec) = require_specifier(value, source)
    {
        push_import_spec(node, relative, &spec, source, out);
        return;
    }
    let Some(name_node) = field(node, "name") else {
        return;
    };
    if name_node.kind() != "identifier" {
        return;
    }
    let name = text(name_node, source).to_owned();
    if let Some(value) = field(node, "value").filter(|n| is_function_value(n.kind())) {
        push_function(
            value,
            full,
            source,
            Some(name),
            None,
            NodeKind::Function,
            out,
        );
        return;
    }
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

fn push_import(node: Node<'_>, relative: &str, source: &[u8], out: &mut Vec<RawSymbol>) {
    let Some(source_node) = field(node, "source") else {
        return;
    };
    push_import_spec(node, relative, text(source_node, source), source, out);
}

fn push_import_spec(
    node: Node<'_>,
    relative: &str,
    spec: &str,
    source: &[u8],
    out: &mut Vec<RawSymbol>,
) {
    let target = import_module(relative, spec, false);
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

fn require_specifier(node: Node<'_>, source: &[u8]) -> Option<String> {
    if node.kind() != "call_expression" {
        return None;
    }
    let function = field(node, "function")?;
    if function.kind() != "identifier" || text(function, source) != "require" {
        return None;
    }
    let args = field(node, "arguments")?;
    let literal = named(args).into_iter().find(|n| n.kind() == "string")?;
    Some(text(literal, source).to_owned())
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
        let declared = type_name(text(type_node, source));
        let name = field(child, "name")
            .or_else(|| field(child, "pattern"))
            .map(|n| text(n, source).to_owned());
        if let Some(name) = name.filter(|n| !n.is_empty() && !n.contains('.'))
            && !declared.is_empty()
        {
            map.insert(name, declared);
        }
    }
    map
}

fn collect_calls(node: Node<'_>, source: &[u8], params: &BTreeMap<String, String>) -> Vec<RawCall> {
    let mut out = Vec::new();
    walk_calls(node, source, params, &mut out);
    dedup_calls(&mut out);
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
    if let Some(call) = read_new(node, source) {
        out.push(call);
    }
    if let Some(call) = read_jsx(node, source) {
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

fn read_new(node: Node<'_>, source: &[u8]) -> Option<RawCall> {
    if node.kind() != "new_expression" {
        return None;
    }
    let constructor = field(node, "constructor")?;
    if constructor.kind() != "identifier" {
        return None;
    }
    let callee = type_name(text(constructor, source));
    if callee.is_empty() {
        return None;
    }
    let (line, _) = lines(node);
    Some(RawCall {
        callee,
        receiver: None,
        receiver_type: None,
        line,
    })
}

fn read_jsx(node: Node<'_>, source: &[u8]) -> Option<RawCall> {
    if !matches!(
        node.kind(),
        "jsx_self_closing_element" | "jsx_opening_element"
    ) {
        return None;
    }
    let name = field(node, "name")?;
    if name.kind() != "identifier" {
        return None;
    }
    let callee = text(name, source);
    let capital = callee
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_uppercase());
    if !capital {
        return None;
    }
    let (line, _) = lines(node);
    Some(RawCall {
        callee: callee.to_owned(),
        receiver: None,
        receiver_type: None,
        line,
    })
}
