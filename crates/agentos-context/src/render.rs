//! Human-readable projections of a Context Pack. JSON stays the canonical form.

use std::fmt::Write as _;

use crate::pack::ContextPack;

/// Markdown pack. Section order matches the JSON fields.
pub fn to_markdown(pack: &ContextPack) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "# Context Pack");
    let _ = writeln!(out);
    let _ = writeln!(out, "Task: {}", pack.task);
    let _ = writeln!(out, "Intent: {} ({})", pack.intent.kind, pack.intent.rule);
    let _ = writeln!(out, "Contexts:");
    for context in &pack.intent.contexts {
        let _ = writeln!(out, "- {context}");
    }
    let budget = pack
        .budget
        .limit
        .map(|limit| limit.to_string())
        .unwrap_or_else(|| "unbounded".to_owned());
    let _ = writeln!(
        out,
        "Budget: {budget} used {} dropped {}",
        pack.budget.used, pack.budget.dropped
    );
    section(
        &mut out,
        "Selected nodes",
        &ids(pack.selected_nodes.iter().map(|node| node.id.as_str())),
    );
    section(
        &mut out,
        "Selected fragments",
        &ids(pack
            .selected_fragments
            .iter()
            .map(|fragment| fragment.id.as_str())),
    );
    section(&mut out, "Terminology", &named(&pack.terminology));
    section(&mut out, "Architecture", &refs(&pack.architecture));
    section(&mut out, "Constraints", &constraint_lines(pack));
    section(&mut out, "Invariants", &refs(&pack.invariants));
    section(&mut out, "Decisions", &refs(&pack.decisions));
    section(&mut out, "Tests", &refs(&pack.tests));
    section(&mut out, "Exclusions", &exclusion_lines(pack));
    out
}

/// Short explanation. The trace remains in the JSON pack.
pub fn explain(pack: &ContextPack) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "task: {}", pack.task);
    let _ = writeln!(out, "intent: {} ({})", pack.intent.kind, pack.intent.rule);
    let _ = writeln!(out, "contexts:");
    if pack.intent.contexts.is_empty() {
        let _ = writeln!(out, "  (none)");
    }
    for context in &pack.intent.contexts {
        let _ = writeln!(out, "  {context}");
    }
    let _ = writeln!(out, "terms:");
    if pack.intent.terms.is_empty() {
        let _ = writeln!(out, "  (none)");
    }
    for term in &pack.intent.terms {
        let _ = writeln!(
            out,
            "  {} -> {} ({})",
            term.task_term, term.node_id, term.context_id
        );
    }
    let budget = pack
        .budget
        .limit
        .map(|limit| limit.to_string())
        .unwrap_or_else(|| "unbounded".to_owned());
    let _ = writeln!(out, "selected_nodes: {}", pack.selected_nodes.len());
    let _ = writeln!(out, "selected_fragments: {}", pack.selected_fragments.len());
    let _ = writeln!(out, "tokens: {}", pack.budget.used);
    let _ = writeln!(out, "budget: {budget}");
    let _ = writeln!(out, "excluded: {}", pack.exclusions.len());
    let mut memory: Vec<&crate::pack::TraceEntry> = pack
        .retrieval_trace
        .entries
        .iter()
        .filter(|entry| entry.item_kind == "node" && entry.id.starts_with("memory."))
        .collect();
    memory.sort_by(|left, right| left.id.cmp(&right.id));
    if !memory.is_empty() {
        let _ = writeln!(out, "memory:");
        for entry in memory {
            let _ = writeln!(out, "  {} {} {}", entry.id, entry.outcome, entry.reason);
        }
    }
    out
}

fn section(out: &mut String, title: &str, body: &[String]) {
    let _ = writeln!(out);
    let _ = writeln!(out, "## {title}");
    if body.is_empty() {
        let _ = writeln!(out, "(none)");
        return;
    }
    for line in body {
        let _ = writeln!(out, "- {line}");
    }
}

fn ids<'a>(values: impl Iterator<Item = &'a str>) -> Vec<String> {
    values.map(str::to_owned).collect()
}

fn named(entries: &[crate::pack::TerminologyEntry]) -> Vec<String> {
    entries
        .iter()
        .map(|entry| format!("{} [{}]", entry.id, entry.status))
        .collect()
}

fn refs(items: &[crate::pack::RefItem]) -> Vec<String> {
    items
        .iter()
        .map(|item| format!("{} [{}]", item.id, item.status))
        .collect()
}

fn constraint_lines(pack: &ContextPack) -> Vec<String> {
    pack.constraints
        .iter()
        .map(|link| format!("{} {} {}", link.from_id, link.relation, link.to_id))
        .collect()
}

fn exclusion_lines(pack: &ContextPack) -> Vec<String> {
    pack.exclusions
        .iter()
        .map(|item| format!("{} {} ({})", item.item_kind, item.id, item.reason))
        .collect()
}
