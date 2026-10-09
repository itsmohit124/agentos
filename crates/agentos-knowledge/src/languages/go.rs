//! Go extractor (tree-sitter).
//!
//! Structs are classes. Interfaces stay interfaces. A struct field with a type and no
//! name is embedding and becomes `bases` (`EXTENDS`, same file only). Go has no
//! `implements` keyword, so implicit satisfaction is not recorded. A composite literal
//! `Payment{}` is a bare call of that type — the Go form of `new Payment()` /
//! `Payment()`. Methods are parented from the receiver type.

use std::collections::BTreeMap;
use std::path::Path;

use tree_sitter::{Node, Parser};

use crate::error::KnowledgeError;
use crate::extract::{LanguageExtractor, RawCall, RawSymbol};
use crate::languages::util::{
    self, dedup_calls, field, import_module, lines, named, text, type_name,
};
use crate::model::NodeKind;

/// Tree-sitter Go extractor, version 1.
#[derive(Debug, Default)]
pub struct GoExtractor;

impl LanguageExtractor for GoExtractor {
    fn name(&self) -> &'static str {
        "go"
    }

    fn version(&self) -> &'static str {
        "1"
    }

    fn matches(&self, path: &Path) -> bool {
        path.extension().and_then(|ext| ext.to_str()) == Some("go")
    }

    fn extract(&self, relative: &str, source: &str) -> Result<Vec<RawSymbol>, KnowledgeError> {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_go::LANGUAGE.into())
            .map_err(|err| KnowledgeError::Extract {
                language: "go",
                message: err.to_string(),
            })?;
        let tree = parser
            .parse(source, None)
            .ok_or_else(|| KnowledgeError::Extract {
                language: "go",
                message: format!("no tree for {relative}"),
            })?;
        let root = tree.root_node();
        if root.has_error() {
            tracing::warn!(
                file = relative,
                "go file has a parse error; extracting what parsed"
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
        "import_declaration" => push_imports(node, relative, source, out),
        "type_declaration" => {
            for child in named(node) {
                if child.kind() == "type_spec" {
                    push_type(child, source, out);
                }
            }
        }
        "function_declaration" => push_function(node, source, None, out),
        "method_declaration" => {
            let parent = field(node, "receiver").and_then(|recv| receiver_type(recv, source));
            push_function(node, source, parent.as_deref(), out);
        }
        "const_declaration" | "var_declaration" => {
            for child in named(node) {
                if matches!(child.kind(), "const_spec" | "var_spec") {
                    push_spec(child, source, out);
                }
            }
        }
        _ => {}
    }
}

fn push_type(node: Node<'_>, source: &[u8], out: &mut Vec<RawSymbol>) {
    let Some(name_node) = field(node, "name") else {
        return;
    };
    let name = text(name_node, source).to_owned();
    if name.is_empty() {
        return;
    }
    let Some(ty) = field(node, "type") else {
        return;
    };
    let kind = match ty.kind() {
        "struct_type" => NodeKind::Class,
        "interface_type" => NodeKind::Interface,
        _ => NodeKind::Symbol,
    };
    let (start, end) = lines(node);
    let body = text(node, source).to_owned();
    let bases = if kind == NodeKind::Class {
        embedded_fields(ty, source)
    } else if kind == NodeKind::Interface {
        embedded_interfaces(ty, source)
    } else {
        Vec::new()
    };
    out.push(RawSymbol {
        kind,
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
    if kind == NodeKind::Interface {
        for child in named(ty) {
            if child.kind() == "method_elem" {
                push_function(child, source, Some(&name), out);
            }
        }
    }
}

fn embedded_fields(struct_type: Node<'_>, source: &[u8]) -> Vec<String> {
    let mut bases = Vec::new();
    for list in named(struct_type) {
        if list.kind() != "field_declaration_list" {
            continue;
        }
        for field_node in named(list) {
            if field_node.kind() != "field_declaration" || field(field_node, "name").is_some() {
                continue;
            }
            if let Some(ty) = field(field_node, "type") {
                let name = go_type_name(ty, source);
                if !name.is_empty() {
                    bases.push(name);
                }
            }
        }
    }
    bases
}

fn embedded_interfaces(interface_type: Node<'_>, source: &[u8]) -> Vec<String> {
    let mut bases = Vec::new();
    for child in named(interface_type) {
        if child.kind() == "type_elem"
            && let Some(name) = first_type_identifier(child, source)
            && !name.is_empty()
        {
            bases.push(name);
        }
    }
    bases
}

fn push_function(node: Node<'_>, source: &[u8], parent: Option<&str>, out: &mut Vec<RawSymbol>) {
    let Some(name_node) = field(node, "name") else {
        return;
    };
    let name = text(name_node, source).to_owned();
    if name.is_empty() {
        return;
    }
    let (start, end) = lines(node);
    let body = text(node, source).to_owned();
    let params = field(node, "parameters")
        .map(|params| parameter_types(params, source))
        .unwrap_or_default();
    let calls = field(node, "body")
        .map(|body_node| collect_calls(body_node, source, &params))
        .unwrap_or_default();
    out.push(RawSymbol {
        kind: if parent.is_some() {
            NodeKind::Method
        } else {
            NodeKind::Function
        },
        name,
        parent: parent.map(str::to_owned),
        start_line: start,
        end_line: end,
        summary: util::summarize("", &body),
        body,
        bases: Vec::new(),
        implements: Vec::new(),
        calls,
        import_target: None,
    });
}

fn push_spec(node: Node<'_>, source: &[u8], out: &mut Vec<RawSymbol>) {
    let Some(name_node) = field(node, "name") else {
        return;
    };
    if name_node.kind() != "identifier" {
        return;
    }
    let name = text(name_node, source).to_owned();
    if name.is_empty() {
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

fn push_imports(node: Node<'_>, relative: &str, source: &[u8], out: &mut Vec<RawSymbol>) {
    let mut specs = Vec::new();
    collect_import_specs(node, &mut specs);
    for spec in specs {
        let Some(path) = field(spec, "path") else {
            continue;
        };
        let target = import_module(relative, text(path, source), false);
        if target.is_empty() {
            continue;
        }
        let (start, end) = lines(spec);
        let body = text(spec, source).to_owned();
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
}

fn collect_import_specs<'t>(node: Node<'t>, out: &mut Vec<Node<'t>>) {
    if node.kind() == "import_spec" {
        out.push(node);
        return;
    }
    for child in named(node) {
        if matches!(child.kind(), "import_spec" | "import_spec_list") {
            collect_import_specs(child, out);
        }
    }
}

fn receiver_type(receiver: Node<'_>, source: &[u8]) -> Option<String> {
    let param = named(receiver)
        .into_iter()
        .find(|child| child.kind() == "parameter_declaration")?;
    let ty = field(param, "type")?;
    let name = go_type_name(ty, source);
    (!name.is_empty()).then_some(name)
}

fn parameter_types(params: Node<'_>, source: &[u8]) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    for child in named(params) {
        if child.kind() != "parameter_declaration" {
            continue;
        }
        let Some(ty) = field(child, "type") else {
            continue;
        };
        let declared = go_type_name(ty, source);
        if declared.is_empty() {
            continue;
        }
        if let Some(name_node) = field(child, "name") {
            let name = text(name_node, source);
            if !name.is_empty() && name != "_" {
                map.insert(name.to_owned(), declared);
            }
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
        "func_literal" | "function_declaration" | "method_declaration" | "type_declaration"
    ) {
        return;
    }
    if node.kind() == "call_expression"
        && let Some(call) = read_call(node, source, params)
    {
        out.push(call);
    }
    if node.kind() == "composite_literal"
        && let Some(ty) = field(node, "type")
        && ty.kind() == "type_identifier"
    {
        let callee = text(ty, source).to_owned();
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

fn read_call(node: Node<'_>, source: &[u8], params: &BTreeMap<String, String>) -> Option<RawCall> {
    let function = field(node, "function")?;
    let (callee, receiver) = match function.kind() {
        "identifier" => (text(function, source).to_owned(), None),
        "selector_expression" => {
            let callee = field(function, "field").map(|n| text(n, source).to_owned())?;
            let receiver = field(function, "operand").filter(|n| n.kind() == "identifier");
            (callee, receiver.map(|n| text(n, source).to_owned()))
        }
        _ => return None,
    };
    if callee.is_empty() {
        return None;
    }
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

fn go_type_name(node: Node<'_>, source: &[u8]) -> String {
    if let Some(name) = first_type_identifier(node, source) {
        return name;
    }
    type_name(text(node, source))
}

fn first_type_identifier(node: Node<'_>, source: &[u8]) -> Option<String> {
    if node.kind() == "type_identifier" {
        let name = text(node, source).to_owned();
        return (!name.is_empty()).then_some(name);
    }
    for child in named(node) {
        if let Some(name) = first_type_identifier(child, source) {
            return Some(name);
        }
    }
    None
}
