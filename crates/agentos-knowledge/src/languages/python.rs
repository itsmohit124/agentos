//! Python extractor (tree-sitter).
//!
//! Module-level classes, functions, imports, and simple assignments become symbols.
//! Methods are functions directly inside a class. Calls are recorded with the receiver's
//! annotated type when the receiver is a parameter. Nested functions are not extracted.

use std::collections::BTreeMap;
use std::path::Path;

use tree_sitter::{Node, Parser};

use crate::error::KnowledgeError;
use crate::extract::{LanguageExtractor, RawCall, RawSymbol};
use crate::languages::util::{self, field, lines, named, text};
use crate::model::NodeKind;

/// Tree-sitter Python extractor, version 1.
#[derive(Debug, Default)]
pub struct PythonExtractor;

impl LanguageExtractor for PythonExtractor {
    fn name(&self) -> &'static str {
        "python"
    }

    fn version(&self) -> &'static str {
        "1"
    }

    fn matches(&self, path: &Path) -> bool {
        path.extension().and_then(|ext| ext.to_str()) == Some("py")
    }

    fn extract(&self, relative: &str, source: &str) -> Result<Vec<RawSymbol>, KnowledgeError> {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_python::LANGUAGE.into())
            .map_err(|err| KnowledgeError::Extract {
                language: "python",
                message: err.to_string(),
            })?;
        let tree = parser
            .parse(source, None)
            .ok_or_else(|| KnowledgeError::Extract {
                language: "python",
                message: format!("no tree for {relative}"),
            })?;
        let root = tree.root_node();
        if root.has_error() {
            tracing::warn!(
                file = relative,
                "python file has a parse error; extracting what parsed"
            );
        }
        let bytes = source.as_bytes();
        let mut symbols = Vec::new();
        for child in named(root) {
            push_statement(child, bytes, None, &mut symbols);
        }
        Ok(symbols)
    }
}

fn push_statement(node: Node<'_>, source: &[u8], parent: Option<&str>, out: &mut Vec<RawSymbol>) {
    match node.kind() {
        "decorated_definition" => {
            if let Some(inner) = field(node, "definition") {
                push_statement(inner, source, parent, out);
            }
        }
        "class_definition" => push_class(node, source, out),
        "function_definition" if parent.is_none() => push_function(node, source, None, out),
        "expression_statement" if parent.is_none() => {
            if let Some(assignment) = named(node).into_iter().find(|n| n.kind() == "assignment") {
                push_assignment(assignment, source, out);
            }
        }
        "import_statement" | "import_from_statement" if parent.is_none() => {
            push_imports(node, source, out);
        }
        _ => {}
    }
}

fn push_class(node: Node<'_>, source: &[u8], out: &mut Vec<RawSymbol>) {
    let Some(name_node) = field(node, "name") else {
        return;
    };
    let name = text(name_node, source).to_owned();
    if name.is_empty() {
        return;
    }
    let (start, end) = lines(node);
    let bases = superclasses(node, source);
    let body = text(node, source).to_owned();
    let summary = util::summarize(
        &docstring(field(node, "body"), source).unwrap_or_default(),
        &body,
    );
    out.push(RawSymbol {
        kind: NodeKind::Class,
        name: name.clone(),
        parent: None,
        start_line: start,
        end_line: end,
        summary,
        body,
        bases,
        implements: Vec::new(),
        calls: Vec::new(),
        import_target: None,
    });
    if let Some(body_node) = field(node, "body") {
        for child in named(body_node) {
            let inner = if child.kind() == "decorated_definition" {
                field(child, "definition")
            } else {
                Some(child)
            };
            if let Some(inner) = inner.filter(|n| n.kind() == "function_definition") {
                push_function(inner, source, Some(&name), out);
            }
        }
    }
}

fn superclasses(node: Node<'_>, source: &[u8]) -> Vec<String> {
    let Some(list) = field(node, "superclasses") else {
        return Vec::new();
    };
    named(list)
        .into_iter()
        .filter(|child| child.kind() == "identifier" || child.kind() == "attribute")
        .map(|child| simple_type_name(text(child, source)))
        .filter(|name| !name.is_empty())
        .collect()
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
    let summary = util::summarize(
        &docstring(field(node, "body"), source).unwrap_or_default(),
        &body,
    );
    let params = parameter_types(field(node, "parameters"), source);
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
        summary,
        body,
        bases: Vec::new(),
        implements: Vec::new(),
        calls,
        import_target: None,
    });
}

fn parameter_types(params: Option<Node<'_>>, source: &[u8]) -> BTreeMap<String, String> {
    let Some(params) = params else {
        return BTreeMap::new();
    };
    let mut map = BTreeMap::new();
    for child in named(params) {
        if !matches!(child.kind(), "typed_parameter" | "typed_default_parameter") {
            continue;
        }
        let Some(type_node) = field(child, "type") else {
            continue;
        };
        let type_name = simple_type_name(text(type_node, source));
        let name = named(child)
            .into_iter()
            .find(|n| n.kind() == "identifier")
            .map(|n| text(n, source).to_owned())
            .or_else(|| field(child, "name").map(|n| text(n, source).to_owned()));
        if let Some(name) = name.filter(|n| !n.is_empty() && n != "self" && n != "cls")
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
        "function_definition" | "class_definition" | "lambda"
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
    let function = field(node, "function")?;
    let (callee, receiver) = match function.kind() {
        "identifier" => (text(function, source).to_owned(), None),
        "attribute" => {
            let callee = field(function, "attribute").map(|n| text(n, source).to_owned())?;
            let receiver = field(function, "object").map(|n| text(n, source).to_owned());
            (callee, receiver)
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

fn push_assignment(node: Node<'_>, source: &[u8], out: &mut Vec<RawSymbol>) {
    let Some(left) = field(node, "left") else {
        return;
    };
    if left.kind() != "identifier" {
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

fn push_imports(node: Node<'_>, source: &[u8], out: &mut Vec<RawSymbol>) {
    let (start, end) = lines(node);
    let body = text(node, source).to_owned();
    let targets: Vec<String> = if node.kind() == "import_from_statement" {
        field(node, "module_name")
            .map(|n| vec![text(n, source).to_owned()])
            .unwrap_or_default()
    } else {
        named(node)
            .into_iter()
            .map(|n| text(n, source).to_owned())
            .collect()
    };
    for target in targets {
        let target = target.trim().trim_start_matches('.').to_owned();
        if target.is_empty() || target.contains('*') {
            continue;
        }
        out.push(RawSymbol {
            kind: NodeKind::Import,
            name: target.clone(),
            parent: None,
            start_line: start,
            end_line: end,
            summary: util::summarize("", &body),
            body: body.clone(),
            bases: Vec::new(),
            implements: Vec::new(),
            calls: Vec::new(),
            import_target: Some(target),
        });
    }
}

fn docstring(body: Option<Node<'_>>, source: &[u8]) -> Option<String> {
    let body = body?;
    let stmt = body.named_child(0)?;
    if stmt.kind() != "expression_statement" {
        return None;
    }
    let string = stmt.named_child(0)?;
    if string.kind() != "string" {
        return None;
    }
    Some(util::unquote_doc(text(string, source)))
}

fn simple_type_name(raw: &str) -> String {
    let raw = raw.trim().trim_start_matches(':').trim();
    let raw = raw.split('[').next().unwrap_or(raw).trim();
    raw.rsplit('.').next().unwrap_or(raw).trim().to_owned()
}
