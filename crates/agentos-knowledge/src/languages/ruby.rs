//! Ruby extractor (tree-sitter).
//!
//! Classes use `<` as `bases` (`EXTENDS`, same file only). A Ruby `module` is stored as
//! a class: it is a named container of methods, and there is no namespace node kind.
//! YARD `@param name [Type]` is a declared parameter type. `Payment.new` is a bare call
//! of `Payment`, the Ruby form of `new Payment()`.

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

/// Tree-sitter Ruby extractor, version 1.
#[derive(Debug, Default)]
pub struct RubyExtractor;

impl LanguageExtractor for RubyExtractor {
    fn name(&self) -> &'static str {
        "ruby"
    }

    fn version(&self) -> &'static str {
        "1"
    }

    fn matches(&self, path: &Path) -> bool {
        path.extension().and_then(|ext| ext.to_str()) == Some("rb")
    }

    fn extract(&self, relative: &str, source: &str) -> Result<Vec<RawSymbol>, KnowledgeError> {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_ruby::LANGUAGE.into())
            .map_err(|err| KnowledgeError::Extract {
                language: "ruby",
                message: err.to_string(),
            })?;
        let tree = parser
            .parse(source, None)
            .ok_or_else(|| KnowledgeError::Extract {
                language: "ruby",
                message: format!("no tree for {relative}"),
            })?;
        let root = tree.root_node();
        if root.has_error() {
            tracing::warn!(
                file = relative,
                "ruby file has a parse error; extracting what parsed"
            );
        }
        let bytes = source.as_bytes();
        let mut symbols = Vec::new();
        for child in named(root) {
            push_stmt(child, relative, source, bytes, None, &mut symbols);
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

fn push_stmt(
    node: Node<'_>,
    relative: &str,
    full: &str,
    source: &[u8],
    parent: Option<&str>,
    out: &mut Vec<RawSymbol>,
) {
    match node.kind() {
        "class" | "module" => push_class(node, relative, full, source, out),
        "method" | "singleton_method" => push_method(node, full, source, parent, out),
        "assignment" if parent.is_none() => push_assignment(node, source, out),
        "call" => {
            if let Some((spec, relative_to_file)) = require_spec(node, source) {
                push_import(node, relative, &spec, relative_to_file, source, out);
            }
        }
        _ => {}
    }
}

fn push_class(node: Node<'_>, relative: &str, full: &str, source: &[u8], out: &mut Vec<RawSymbol>) {
    let Some(name_node) = field(node, "name") else {
        return;
    };
    let name = type_name(text(name_node, source));
    if name.is_empty() {
        return;
    }
    let (start, end) = lines(node);
    let mut bases = Vec::new();
    if let Some(superclass) = field(node, "superclass") {
        for child in named(superclass) {
            let base = type_name(text(child, source));
            if !base.is_empty() {
                bases.push(base);
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
        implements: Vec::new(),
        calls: Vec::new(),
        import_target: None,
    });
    if let Some(body_node) = field(node, "body") {
        for child in named(body_node) {
            push_stmt(child, relative, full, source, Some(&name), out);
        }
    }
}

fn push_method(
    node: Node<'_>,
    full: &str,
    source: &[u8],
    parent: Option<&str>,
    out: &mut Vec<RawSymbol>,
) {
    let Some(name_node) = field(node, "name") else {
        return;
    };
    let name = text(name_node, source).to_owned();
    if name.is_empty() {
        return;
    }
    let (start, end) = lines(node);
    let body_text = text(node, source).to_owned();
    let params = comment_param_types(&attached_comment(node, full));
    let calls = field(node, "body")
        .map(|body| collect_calls(body, source, &params))
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
        summary: util::summarize("", &body_text),
        body: body_text,
        bases: Vec::new(),
        implements: Vec::new(),
        calls,
        import_target: None,
    });
}

fn push_assignment(node: Node<'_>, source: &[u8], out: &mut Vec<RawSymbol>) {
    let Some(left) = field(node, "left") else {
        return;
    };
    if !matches!(left.kind(), "identifier" | "constant") {
        return;
    }
    let name = text(left, source).to_owned();
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

fn push_import(
    node: Node<'_>,
    relative: &str,
    spec: &str,
    force_relative: bool,
    source: &[u8],
    out: &mut Vec<RawSymbol>,
) {
    let target = import_module(relative, spec, force_relative);
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

fn require_spec(node: Node<'_>, source: &[u8]) -> Option<(String, bool)> {
    let method = field(node, "method")?;
    let name = text(method, source);
    if name != "require" && name != "require_relative" {
        return None;
    }
    let args = field(node, "arguments")?;
    let literal = named(args)
        .into_iter()
        .find(|child| child.kind() == "string")?;
    Some((text(literal, source).to_owned(), name == "require_relative"))
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
        "method" | "singleton_method" | "class" | "module" | "lambda" | "do_block" | "block"
    ) {
        return;
    }
    if node.kind() == "call"
        && let Some(call) = read_call(node, source, params)
    {
        out.push(call);
    }
    for child in named(node) {
        walk_calls(child, source, params, out);
    }
}

fn read_call(node: Node<'_>, source: &[u8], params: &BTreeMap<String, String>) -> Option<RawCall> {
    let method = field(node, "method")?;
    let callee = text(method, source).to_owned();
    if callee.is_empty() || callee == "require" || callee == "require_relative" {
        return None;
    }
    let receiver_node = field(node, "receiver");
    if callee == "new"
        && let Some(receiver) = receiver_node
        && matches!(receiver.kind(), "constant" | "scope_resolution")
    {
        let constructed = type_name(text(receiver, source));
        if constructed.is_empty() {
            return None;
        }
        let (line, _) = lines(node);
        return Some(RawCall {
            callee: constructed,
            receiver: None,
            receiver_type: None,
            line,
        });
    }
    let receiver = receiver_node
        .filter(|receiver| receiver.kind() == "identifier")
        .map(|receiver| text(receiver, source).to_owned());
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
