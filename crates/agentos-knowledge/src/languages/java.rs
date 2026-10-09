//! Java extractor (tree-sitter).
//!
//! Classes and interfaces map onto the existing node kinds. `extends` becomes `bases`
//! and `implements` becomes `implements` (both link only inside the same file).
//! `new Payment()` is a bare call of `Payment`. Test methods are tests only when their
//! name starts with `test_`, which is the Phase 2 rule for methods.

use std::collections::BTreeMap;
use std::path::Path;

use tree_sitter::{Node, Parser};

use crate::error::KnowledgeError;
use crate::extract::{LanguageExtractor, RawCall, RawSymbol};
use crate::languages::util::{
    self, dedup_calls, field, import_module, lines, named, text, type_name,
};
use crate::model::NodeKind;

/// Tree-sitter Java extractor, version 1.
#[derive(Debug, Default)]
pub struct JavaExtractor;

impl LanguageExtractor for JavaExtractor {
    fn name(&self) -> &'static str {
        "java"
    }

    fn version(&self) -> &'static str {
        "1"
    }

    fn matches(&self, path: &Path) -> bool {
        path.extension().and_then(|ext| ext.to_str()) == Some("java")
    }

    fn extract(&self, relative: &str, source: &str) -> Result<Vec<RawSymbol>, KnowledgeError> {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_java::LANGUAGE.into())
            .map_err(|err| KnowledgeError::Extract {
                language: "java",
                message: err.to_string(),
            })?;
        let tree = parser
            .parse(source, None)
            .ok_or_else(|| KnowledgeError::Extract {
                language: "java",
                message: format!("no tree for {relative}"),
            })?;
        let root = tree.root_node();
        if root.has_error() {
            tracing::warn!(
                file = relative,
                "java file has a parse error; extracting what parsed"
            );
        }
        let bytes = source.as_bytes();
        let mut symbols = Vec::new();
        for child in named(root) {
            push_item(child, relative, bytes, &mut symbols);
        }
        order_parents_first(&mut symbols);
        Ok(symbols)
    }
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

fn push_item(node: Node<'_>, relative: &str, source: &[u8], out: &mut Vec<RawSymbol>) {
    match node.kind() {
        "import_declaration" => push_import(node, relative, source, out),
        "class_declaration" => push_class(node, source, out),
        "interface_declaration" => push_interface(node, source, out),
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
    if let Some(superclass) = field(node, "superclass") {
        collect_simple_types(superclass, source, &mut bases);
    }
    if let Some(interfaces) = field(node, "interfaces") {
        collect_simple_types(interfaces, source, &mut implements);
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
                "method_declaration" | "constructor_declaration" => {
                    push_method(child, source, &name, out);
                }
                _ => {}
            }
        }
    }
}

fn push_interface(node: Node<'_>, source: &[u8], out: &mut Vec<RawSymbol>) {
    let Some(name) = field(node, "name").map(|n| text(n, source).to_owned()) else {
        return;
    };
    if name.is_empty() {
        return;
    }
    let (start, end) = lines(node);
    let mut bases = Vec::new();
    for child in named(node) {
        if child.kind() == "extends_interfaces" {
            collect_simple_types(child, source, &mut bases);
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
            if child.kind() == "method_declaration" {
                push_method(child, source, &name, out);
            }
        }
    }
}

fn push_method(node: Node<'_>, source: &[u8], parent: &str, out: &mut Vec<RawSymbol>) {
    let Some(name) = field(node, "name").map(|n| text(n, source).to_owned()) else {
        return;
    };
    if name.is_empty() {
        return;
    }
    let (start, end) = lines(node);
    let body_text = text(node, source).to_owned();
    let params = field(node, "parameters")
        .map(|params| parameter_types(params, source))
        .unwrap_or_default();
    let calls = field(node, "body")
        .map(|body| collect_calls(body, source, &params))
        .unwrap_or_default();
    out.push(RawSymbol {
        kind: NodeKind::Method,
        name,
        parent: Some(parent.to_owned()),
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

fn push_import(node: Node<'_>, relative: &str, source: &[u8], out: &mut Vec<RawSymbol>) {
    if named(node).iter().any(|child| child.kind() == "asterisk") {
        return;
    }
    let Some(target_node) = named(node)
        .into_iter()
        .find(|child| matches!(child.kind(), "scoped_identifier" | "identifier"))
    else {
        return;
    };
    let target = import_module(relative, text(target_node, source), false);
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

fn parameter_types(params: Node<'_>, source: &[u8]) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    for child in named(params) {
        if child.kind() != "formal_parameter" {
            continue;
        }
        let Some(ty) = field(child, "type") else {
            continue;
        };
        let declared = type_name(text(ty, source));
        let Some(name_node) = field(child, "name") else {
            continue;
        };
        let name = text(name_node, source);
        if !name.is_empty() && !declared.is_empty() && !name.contains('.') {
            map.insert(name.to_owned(), declared);
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
        "class_declaration"
            | "interface_declaration"
            | "method_declaration"
            | "constructor_declaration"
            | "lambda_expression"
            | "class_body"
    ) {
        return;
    }
    if node.kind() == "method_invocation"
        && let Some(call) = read_invocation(node, source, params)
    {
        out.push(call);
    }
    if node.kind() == "object_creation_expression"
        && let Some(ty) = field(node, "type")
    {
        let callee = type_name(text(ty, source));
        if !callee.is_empty() {
            let (line, _) = lines(node);
            out.push(RawCall {
                callee,
                receiver: None,
                receiver_type: None,
                line,
            });
        }
    }
    for child in named(node) {
        walk_calls(child, source, params, out);
    }
}

fn read_invocation(
    node: Node<'_>,
    source: &[u8],
    params: &BTreeMap<String, String>,
) -> Option<RawCall> {
    let callee = field(node, "name").map(|n| text(n, source).to_owned())?;
    if callee.is_empty() {
        return None;
    }
    let receiver = field(node, "object").filter(|object| object.kind() == "identifier");
    let receiver = receiver.map(|object| text(object, source).to_owned());
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

fn collect_simple_types(node: Node<'_>, source: &[u8], out: &mut Vec<String>) {
    match node.kind() {
        "type_identifier" | "identifier" => {
            let name = text(node, source).trim().to_owned();
            if !name.is_empty() {
                out.push(name);
            }
        }
        "scoped_identifier" | "generic_type" => {
            let name = type_name(text(node, source));
            if !name.is_empty() {
                out.push(name);
            }
        }
        _ => {
            for child in named(node) {
                collect_simple_types(child, source, out);
            }
        }
    }
}
