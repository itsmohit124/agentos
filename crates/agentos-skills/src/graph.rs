//! The skill relationship graph and its deterministic text / DOT renderings.

use std::fmt::Write as _;

use serde::Serialize;

use crate::loader::Level;
use crate::manifest::SkillKind;
use crate::registry::SkillRegistry;

/// Kind of relationship between two skills.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum EdgeKind {
    /// `requires` (execution dependency).
    Requires,
    /// `optional_recommends`.
    Recommends,
    /// Router `routes`.
    Routes,
}

impl EdgeKind {
    /// Stable name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Requires => "requires",
            Self::Recommends => "recommends",
            Self::Routes => "routes",
        }
    }
}

/// A skill in the graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GraphNode {
    /// Name.
    pub name: String,
    /// Version.
    pub version: String,
    /// Kind.
    pub kind: SkillKind,
    /// Discovery level.
    pub level: Level,
}

/// A relationship.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct GraphEdge {
    /// Source skill.
    pub from: String,
    /// Relationship kind.
    pub kind: EdgeKind,
    /// Target skill.
    pub to: String,
    /// Version constraint (requires only).
    pub constraint: Option<String>,
    /// Whether the target is not installed.
    pub missing: bool,
}

/// Skills and their relationships, sorted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SkillGraph {
    /// Nodes sorted by name.
    pub nodes: Vec<GraphNode>,
    /// Edges sorted by (from, kind, to).
    pub edges: Vec<GraphEdge>,
}

impl SkillGraph {
    /// Builds the graph of `registry`.
    pub fn from_registry(registry: &SkillRegistry) -> Self {
        let nodes = registry
            .iter()
            .map(|s| GraphNode {
                name: s.name().to_owned(),
                version: s.manifest().version.to_string(),
                kind: s.manifest().kind,
                level: s.level(),
            })
            .collect();
        let mut edges = Vec::new();
        for skill in registry.iter() {
            let m = skill.manifest();
            let edge = |kind, to: &str, constraint| GraphEdge {
                from: skill.name().to_owned(),
                kind,
                to: to.to_owned(),
                constraint,
                missing: registry.get(to).is_none(),
            };
            for dep in &m.requires {
                edges.push(edge(
                    EdgeKind::Requires,
                    &dep.name,
                    dep.version.as_ref().map(ToString::to_string),
                ));
            }
            for rec in &m.optional_recommends {
                edges.push(edge(EdgeKind::Recommends, rec, None));
            }
            for route in &m.routes {
                edges.push(edge(EdgeKind::Routes, route, None));
            }
        }
        edges.sort();
        Self { nodes, edges }
    }

    /// Plain-text rendering: one block per skill with its outgoing edges.
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(
            out,
            "Skill graph: {} skills, {} edges",
            self.nodes.len(),
            self.edges.len()
        );
        for node in &self.nodes {
            let kind = match node.kind {
                SkillKind::Router => ", router",
                SkillKind::Skill => "",
            };
            let _ = writeln!(out, "{} {} [{}{kind}]", node.name, node.version, node.level);
            for edge in self.edges.iter().filter(|e| e.from == node.name) {
                let constraint = edge
                    .constraint
                    .as_ref()
                    .map(|c| format!(" {c}"))
                    .unwrap_or_default();
                let missing = if edge.missing { " (missing)" } else { "" };
                let _ = writeln!(
                    out,
                    "  {} -> {}{constraint}{missing}",
                    edge.kind.as_str(),
                    edge.to
                );
            }
        }
        out
    }

    /// Graphviz DOT rendering.
    pub fn to_dot(&self) -> String {
        let mut out = String::from("digraph skills {\n  rankdir=LR;\n  node [shape=box];\n");
        for node in &self.nodes {
            let shape = match node.kind {
                SkillKind::Router => ", shape=diamond",
                SkillKind::Skill => "",
            };
            let _ = writeln!(
                out,
                "  {:?} [label=\"{}\\n{} ({})\"{shape}];",
                node.name, node.name, node.version, node.level
            );
        }
        let missing: std::collections::BTreeSet<&str> = self
            .edges
            .iter()
            .filter(|e| e.missing)
            .map(|e| e.to.as_str())
            .collect();
        for name in missing {
            let _ = writeln!(
                out,
                "  {name:?} [label=\"{name}\\n(missing)\", style=dashed, color=red];"
            );
        }
        for edge in &self.edges {
            let style = match edge.kind {
                EdgeKind::Requires => "solid",
                EdgeKind::Recommends => "dashed",
                EdgeKind::Routes => "dotted",
            };
            let label = match &edge.constraint {
                Some(c) => format!("{} {c}", edge.kind.as_str()),
                None => edge.kind.as_str().to_owned(),
            };
            let _ = writeln!(
                out,
                "  {:?} -> {:?} [label={label:?}, style={style}];",
                edge.from, edge.to
            );
        }
        out.push_str("}\n");
        out
    }
}
