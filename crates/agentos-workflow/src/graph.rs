//! Task-graph preparation: cycles, skip collapse, and ready layers.
//!
//! Skipped steps stay in the definition. Their dependents wait on the nearest
//! included ancestors instead. A cycle fails before any step runs.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::definition::{StepSpec, WorkflowDefinition};
use crate::error::WorkflowError;

/// An included step and the dependencies that remain after skips.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncludedStep {
    /// Step id.
    pub id: String,
    /// Summary.
    pub summary: String,
    /// Skill name, when the step selects one.
    pub skill: Option<String>,
    /// Included steps this one waits for.
    pub depends_on: Vec<String>,
    /// Consumed output names.
    pub consumes: Vec<String>,
    /// Produced output names.
    pub outputs: Vec<String>,
    /// Validation kind.
    pub validation: Option<String>,
    /// Declared capabilities.
    pub permissions: Vec<String>,
    /// Fix loop enabled.
    pub fix_loop: bool,
    /// Other steps that become ready in the same layer.
    pub ready_with: Vec<String>,
}

/// A skipped step and the reason it is not run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkippedStep {
    /// Step id.
    pub id: String,
    /// Deterministic reason from the definition.
    pub reason: String,
}

/// The executable graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedGraph {
    /// Included steps in topological order. Ties use the step id.
    pub steps: Vec<IncludedStep>,
    /// Skipped steps, sorted by id.
    pub skipped: Vec<SkippedStep>,
    /// Original edges, including edges that touch a skipped step.
    pub task_edges: Vec<(String, String)>,
    /// Edges among included steps.
    pub active_edges: Vec<(String, String)>,
}

/// Validates `definition` and builds the executable graph.
pub fn prepare(definition: &WorkflowDefinition) -> Result<PreparedGraph, WorkflowError> {
    let mut seen = BTreeSet::new();
    let mut by_id: BTreeMap<&str, &StepSpec> = BTreeMap::new();
    for step in &definition.steps {
        if !valid_step_id(&step.id) {
            return Err(WorkflowError::Definition(format!(
                "step id {:?} is not a safe identifier",
                step.id
            )));
        }
        if !seen.insert(step.id.as_str()) {
            return Err(WorkflowError::Definition(format!(
                "duplicate step id {:?}",
                step.id
            )));
        }
        by_id.insert(step.id.as_str(), step);
    }
    let mut task_edges = Vec::new();
    for step in &definition.steps {
        for dep in &step.depends_on {
            if !by_id.contains_key(dep.as_str()) {
                return Err(WorkflowError::Definition(format!(
                    "step {:?} depends on unknown step {dep:?}",
                    step.id
                )));
            }
            task_edges.push((dep.clone(), step.id.clone()));
        }
    }
    task_edges.sort();
    task_edges.dedup();
    detect_cycle(&by_id)?;

    let mut skipped = Vec::new();
    let mut included_ids = BTreeSet::new();
    for step in &definition.steps {
        if let Some(reason) = &step.skip_reason {
            let reason = reason.trim();
            if reason.is_empty() {
                return Err(WorkflowError::Definition(format!(
                    "step {:?} has an empty skip reason",
                    step.id
                )));
            }
            skipped.push(SkippedStep {
                id: step.id.clone(),
                reason: reason.to_owned(),
            });
        } else {
            included_ids.insert(step.id.clone());
        }
    }
    if included_ids.is_empty() {
        return Err(WorkflowError::Definition(format!(
            "workflow {:?} has no steps to run",
            definition.id
        )));
    }
    skipped.sort_by(|left, right| left.id.cmp(&right.id));

    let mut collapsed: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for id in &included_ids {
        collapsed.insert(id.clone(), ancestors(id, &by_id, &included_ids));
    }
    let mut active_edges = Vec::new();
    for (id, deps) in &collapsed {
        for dep in deps {
            active_edges.push((dep.clone(), id.clone()));
        }
    }
    active_edges.sort();

    let layers = layers_of(&included_ids, &collapsed)?;
    let mut ready_with = BTreeMap::new();
    for layer in &layers {
        for id in layer {
            ready_with.insert(id.clone(), layer.clone());
        }
    }

    let mut steps = Vec::new();
    for layer in &layers {
        for id in layer {
            let spec = by_id[id.as_str()];
            let mut depends_on: Vec<String> = collapsed[id].iter().cloned().collect();
            depends_on.sort();
            steps.push(IncludedStep {
                id: id.clone(),
                summary: spec.summary.clone(),
                skill: spec.skill.clone(),
                depends_on,
                consumes: spec.consumes.clone(),
                outputs: spec.outputs.clone(),
                validation: spec.validation.clone(),
                permissions: spec.permissions.clone(),
                fix_loop: spec.fix_loop,
                ready_with: ready_with[id].clone(),
            });
        }
    }

    Ok(PreparedGraph {
        steps,
        skipped,
        task_edges,
        active_edges,
    })
}

fn valid_step_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
}

fn ancestors(
    id: &str,
    by_id: &BTreeMap<&str, &StepSpec>,
    included: &BTreeSet<String>,
) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut stack: Vec<String> = by_id[id].depends_on.clone();
    let mut seen = BTreeSet::new();
    while let Some(dep) = stack.pop() {
        if !seen.insert(dep.clone()) {
            continue;
        }
        if included.contains(&dep) {
            out.insert(dep);
        } else if let Some(spec) = by_id.get(dep.as_str()) {
            stack.extend(spec.depends_on.iter().cloned());
        }
    }
    out
}

fn layers_of(
    included: &BTreeSet<String>,
    deps: &BTreeMap<String, BTreeSet<String>>,
) -> Result<Vec<Vec<String>>, WorkflowError> {
    let mut finished = BTreeSet::new();
    let mut remaining = included.clone();
    let mut layers = Vec::new();
    while !remaining.is_empty() {
        let mut layer: Vec<String> = remaining
            .iter()
            .filter(|id| deps[*id].iter().all(|dep| finished.contains(dep)))
            .cloned()
            .collect();
        if layer.is_empty() {
            return Err(WorkflowError::Definition(
                "included steps have a cycle after skip collapse".to_owned(),
            ));
        }
        layer.sort();
        for id in &layer {
            remaining.remove(id);
            finished.insert(id.clone());
        }
        layers.push(layer);
    }
    Ok(layers)
}

fn detect_cycle(by_id: &BTreeMap<&str, &StepSpec>) -> Result<(), WorkflowError> {
    let mut marks: BTreeMap<&str, Mark> = BTreeMap::new();
    let mut stack = Vec::new();
    for id in by_id.keys().copied() {
        visit(id, by_id, &mut marks, &mut stack)?;
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mark {
    Visiting,
    Done,
}

fn visit<'a>(
    id: &'a str,
    by_id: &BTreeMap<&'a str, &'a StepSpec>,
    marks: &mut BTreeMap<&'a str, Mark>,
    stack: &mut Vec<&'a str>,
) -> Result<(), WorkflowError> {
    match marks.get(id) {
        Some(Mark::Done) => return Ok(()),
        Some(Mark::Visiting) => {
            let start = stack.iter().position(|item| *item == id).unwrap_or(0);
            let mut path: Vec<String> = stack[start..]
                .iter()
                .map(|item| (*item).to_owned())
                .collect();
            path.push(id.to_owned());
            return Err(WorkflowError::Cycle {
                path: canonical_cycle(&path),
            });
        }
        None => {}
    }
    marks.insert(id, Mark::Visiting);
    stack.push(id);
    let mut deps: Vec<&str> = by_id[id].depends_on.iter().map(String::as_str).collect();
    deps.sort_unstable();
    for dep in deps {
        visit(dep, by_id, marks, stack)?;
    }
    stack.pop();
    marks.insert(id, Mark::Done);
    Ok(())
}

fn canonical_cycle(path: &[String]) -> Vec<String> {
    if path.len() < 2 {
        return path.to_vec();
    }
    let ring = &path[..path.len() - 1];
    let start = ring
        .iter()
        .enumerate()
        .min_by(|left, right| left.1.cmp(right.1))
        .map_or(0, |(index, _)| index);
    let mut out: Vec<String> = ring[start..]
        .iter()
        .chain(&ring[..start])
        .cloned()
        .collect();
    if let Some(first) = out.first().cloned() {
        out.push(first);
    }
    out
}
