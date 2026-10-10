//! Deterministic plans.
//!
//! The task text is stored as given. `--workflow` selects the procedure. When it is
//! omitted, the context engine's intent words select `bugfix`, `refactor`, or `feature`.
//! That choice is recorded. It does not rewrite the task.

use std::collections::BTreeSet;

use agentos_context::intent_kind;
use agentos_feedback::ValidationKind;
use agentos_permissions::{Capability, PermissionSubject};
use agentos_skills::SkillRegistry;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::definition::{WorkflowDefinition, load};
use crate::error::WorkflowError;
use crate::graph::{self, SkippedStep};

/// How the workflow was chosen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Selection {
    /// `explicit` or `task-type`.
    pub source: String,
    /// The rule that fired.
    pub rule: String,
}

/// One skill the plan needs, with the reason it was selected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectedSkill {
    /// Skill name.
    pub name: String,
    /// Installed version.
    pub version: String,
    /// Step that caused the selection.
    pub step_id: String,
    /// Resolver position within that step. Dependencies come first.
    pub position: u32,
    /// Why this skill is on the plan.
    pub reason: String,
    /// Capabilities the skill declares, sorted.
    pub permissions: Vec<String>,
}

/// A skill dependency edge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillEdge {
    /// The skill that declares the requirement.
    pub from: String,
    /// The required skill.
    pub to: String,
    /// Step whose resolution produced the edge.
    pub step_id: String,
}

/// A task-graph edge. `active` is false when either end is skipped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edge {
    /// Dependency that runs first.
    pub from: String,
    /// Dependent step.
    pub to: String,
    /// Whether both ends are executed.
    pub active: bool,
}

/// Validation the plan will ask Phase 5 to perform.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationRequirement {
    /// Step id.
    pub step_id: String,
    /// Kind wire name.
    pub kind: String,
    /// Whether failure may enter the fix loop.
    pub fix_loop: bool,
    /// Maximum validation attempts.
    pub max_iterations: u32,
}

/// Scheduling facts that do not depend on a clock.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionMeta {
    /// Planner version.
    pub planner: String,
    /// How ordered steps are produced.
    pub ordering: String,
    /// Ready-frontier admission limit.
    pub concurrency: u32,
    /// Fix-loop attempt cap.
    pub max_fix_iterations: u32,
}

/// One executable step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlannedStep {
    /// Stable id.
    pub id: String,
    /// What the step is for.
    pub summary: String,
    /// Skill named by the step, when it has one.
    pub skill: Option<String>,
    /// Included steps this one waits for.
    pub depends_on: Vec<String>,
    /// Output names taken from those steps.
    pub consumes: Vec<String>,
    /// Output names this step produces.
    pub outputs: Vec<String>,
    /// Validation kind.
    pub validation: Option<String>,
    /// Fix loop enabled.
    pub fix_loop: bool,
    /// Capabilities the step declares.
    pub granted: Vec<String>,
    /// Capabilities that must be declared before the runner starts.
    pub required: Vec<String>,
    /// Steps that become ready in the same layer, including this one.
    pub ready_with: Vec<String>,
}

/// A deterministic plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    /// Content hash of the plan body.
    pub id: String,
    /// The user task, unchanged.
    pub task: String,
    /// Workflow id.
    pub workflow: String,
    /// Task type of that workflow.
    pub task_type: String,
    /// Why this workflow was selected.
    pub selection: Selection,
    /// Included steps in topological order.
    pub steps: Vec<PlannedStep>,
    /// Active and inactive task edges.
    pub dependencies: Vec<Edge>,
    /// Skill requirement edges.
    pub skill_edges: Vec<SkillEdge>,
    /// Selected skills, sorted by step id then resolver position.
    pub skills: Vec<SelectedSkill>,
    /// Steps that do not run, each with a reason.
    pub skipped: Vec<SkippedStep>,
    /// Capabilities required by the plan, sorted.
    pub permissions: Vec<String>,
    /// Validation requirements.
    pub validation: Vec<ValidationRequirement>,
    /// Scheduler settings.
    pub execution: ExecutionMeta,
}

/// Inputs that select a plan. The same inputs produce the same plan.
pub struct PlanInput<'a> {
    /// User task. Stored verbatim.
    pub task: &'a str,
    /// `--workflow` value. `None` selects from task-type words.
    pub workflow: Option<&'a str>,
    /// A definition supplied by a test. When set, `workflow` is not consulted.
    pub definition: Option<&'a WorkflowDefinition>,
    /// Phase 1 registry. Skill dependencies are resolved here.
    pub registry: &'a SkillRegistry,
    /// Replaces the definition's concurrency.
    pub concurrency: Option<u32>,
    /// Replaces the definition's fix-loop cap.
    pub max_fix_iterations: Option<u32>,
}

/// Builds a plan.
pub fn plan(input: &PlanInput<'_>) -> Result<Plan, WorkflowError> {
    let (definition, selection) = match input.definition {
        Some(definition) => (
            definition.clone(),
            Selection {
                source: "definition".to_owned(),
                rule: format!("supplied:{}", definition.id),
            },
        ),
        None => select(input.task, input.workflow)?,
    };
    let mut definition = definition;
    if let Some(concurrency) = input.concurrency {
        if concurrency == 0 {
            return Err(WorkflowError::UnboundedConcurrency);
        }
        definition.concurrency = concurrency;
    }
    if let Some(limit) = input.max_fix_iterations {
        definition.max_fix_iterations = limit;
    }
    let prepared = graph::prepare(&definition)?;
    let mut skills = Vec::new();
    let mut skill_edges = Vec::new();
    let mut steps = Vec::new();
    let mut validation = Vec::new();
    let mut permissions = BTreeSet::new();

    for step in &prepared.steps {
        let selected = match &step.skill {
            Some(name) => {
                let (selected, edges) = resolve_skills(input.registry, &step.id, name)?;
                skill_edges.extend(edges);
                selected
            }
            None => Vec::new(),
        };
        let mut granted = BTreeSet::new();
        for skill in &selected {
            for cap in &skill.permissions {
                granted.insert(cap.clone());
            }
        }
        for cap in parse_caps(&step.permissions)? {
            granted.insert(cap.as_str().to_owned());
        }
        let mut required = granted.clone();
        if let Some(kind) = &step.validation {
            let parsed: ValidationKind = kind
                .parse()
                .map_err(|err: String| WorkflowError::Definition(err))?;
            let extra = match parsed {
                ValidationKind::Architecture => Capability::FsRead,
                ValidationKind::UnitTest
                | ValidationKind::IntegrationTest
                | ValidationKind::Typecheck
                | ValidationKind::Lint
                | ValidationKind::Build => Capability::Shell,
            };
            required.insert(extra.as_str().to_owned());
            validation.push(ValidationRequirement {
                step_id: step.id.clone(),
                kind: parsed.as_str().to_owned(),
                fix_loop: step.fix_loop,
                max_iterations: definition.max_fix_iterations,
            });
        }
        for cap in &required {
            permissions.insert(cap.clone());
        }
        skills.extend(selected);
        steps.push(PlannedStep {
            id: step.id.clone(),
            summary: step.summary.clone(),
            skill: step.skill.clone(),
            depends_on: step.depends_on.clone(),
            consumes: step.consumes.clone(),
            outputs: step.outputs.clone(),
            validation: step.validation.clone(),
            fix_loop: step.fix_loop,
            granted: granted.into_iter().collect(),
            required: required.into_iter().collect(),
            ready_with: step.ready_with.clone(),
        });
    }
    skills.sort_by(|left, right| {
        left.step_id
            .cmp(&right.step_id)
            .then(left.position.cmp(&right.position))
            .then(left.name.cmp(&right.name))
    });
    skill_edges.sort_by(|left, right| {
        left.step_id
            .cmp(&right.step_id)
            .then(left.from.cmp(&right.from))
            .then(left.to.cmp(&right.to))
    });
    validation.sort_by(|left, right| left.step_id.cmp(&right.step_id));

    let skipped_ids: BTreeSet<&str> = prepared
        .skipped
        .iter()
        .map(|step| step.id.as_str())
        .collect();
    let mut dependencies: Vec<Edge> = prepared
        .task_edges
        .iter()
        .map(|(from, to)| Edge {
            from: from.clone(),
            to: to.clone(),
            active: !skipped_ids.contains(from.as_str()) && !skipped_ids.contains(to.as_str()),
        })
        .collect();
    dependencies.sort_by(|left, right| {
        left.from
            .cmp(&right.from)
            .then(left.to.cmp(&right.to))
            .then(left.active.cmp(&right.active))
    });

    let mut plan = Plan {
        id: String::new(),
        task: input.task.to_owned(),
        workflow: definition.id.clone(),
        task_type: definition.task_type.clone(),
        selection,
        steps,
        dependencies,
        skill_edges,
        skills,
        skipped: prepared.skipped,
        permissions: permissions.into_iter().collect(),
        validation,
        execution: ExecutionMeta {
            planner: "deterministic-v1".to_owned(),
            ordering: "topological-by-step-id".to_owned(),
            concurrency: definition.concurrency,
            max_fix_iterations: definition.max_fix_iterations,
        },
    };
    let body = serde_json::to_string(&plan).map_err(|err| WorkflowError::Store(err.to_string()))?;
    plan.id = format!("{}-{}", plan.workflow, &hex_sha256(&body)[..16]);
    Ok(plan)
}

fn select(
    task: &str,
    workflow: Option<&str>,
) -> Result<(WorkflowDefinition, Selection), WorkflowError> {
    if let Some(name) = workflow {
        return Ok((
            load(name)?,
            Selection {
                source: "explicit".to_owned(),
                rule: format!("flag:{name}"),
            },
        ));
    }
    let (kind, rule) = intent_kind(task);
    let name = match kind.as_str() {
        "bugfix" => "bugfix",
        "refactor" => "refactor",
        _ => "feature",
    };
    Ok((
        load(name)?,
        Selection {
            source: "task-type".to_owned(),
            rule: format!("{rule} -> {name}"),
        },
    ))
}

fn resolve_skills(
    registry: &SkillRegistry,
    step_id: &str,
    skill_name: &str,
) -> Result<(Vec<SelectedSkill>, Vec<SkillEdge>), WorkflowError> {
    let order = registry.resolve(skill_name)?;
    let mut edges = Vec::new();
    for skill in &order {
        for dep in &skill.manifest().requires {
            if order.iter().any(|other| other.name() == dep.name) {
                edges.push(SkillEdge {
                    from: skill.name().to_owned(),
                    to: dep.name.clone(),
                    step_id: step_id.to_owned(),
                });
            }
        }
    }
    let mut selected = Vec::new();
    for (position, skill) in order.iter().enumerate() {
        let reason = if skill.name() == skill_name {
            format!("selected by step {step_id}")
        } else {
            let parent = order[position + 1..].iter().find(|later| {
                later
                    .manifest()
                    .requires
                    .iter()
                    .any(|dep| dep.name == skill.name())
            });
            match parent {
                Some(parent) => format!(
                    "required by step {step_id} because {} requires {}",
                    parent.name(),
                    skill.name()
                ),
                None => format!(
                    "required by step {step_id} because {skill_name} requires {}",
                    skill.name()
                ),
            }
        };
        selected.push(SelectedSkill {
            name: skill.name().to_owned(),
            version: skill.manifest().version.to_string(),
            step_id: step_id.to_owned(),
            position: u32::try_from(position).unwrap_or(u32::MAX),
            reason,
            permissions: skill
                .permissions()
                .iter()
                .map(|cap| cap.as_str().to_owned())
                .collect(),
        });
    }
    Ok((selected, edges))
}

fn parse_caps(names: &[String]) -> Result<Vec<Capability>, WorkflowError> {
    let mut caps = Vec::new();
    for name in names {
        caps.push(
            name.parse()
                .map_err(|err: agentos_permissions::UnknownCapability| {
                    WorkflowError::Definition(err.to_string())
                })?,
        );
    }
    Ok(caps)
}

fn hex_sha256(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
