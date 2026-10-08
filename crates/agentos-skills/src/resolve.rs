//! Dependency resolution: deterministic topological order or a deterministic error.

use std::collections::{BTreeMap, BTreeSet};

use crate::diagnostic::{Diagnostic, codes};
use crate::registry::SkillRegistry;

/// Why a dependency order could not be produced.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ResolveError {
    /// The requested skill is not registered.
    #[error("unknown skill {0:?}")]
    UnknownSkill(String),
    /// A required skill is not installed.
    #[error("skill {skill:?} requires {dependency:?}, which is not installed")]
    MissingDependency {
        /// The skill declaring the dependency.
        skill: String,
        /// The missing skill.
        dependency: String,
    },
    /// A required skill is installed at a version outside the constraint.
    #[error("skill {skill:?} requires {dependency} {required}, but {found} is installed")]
    VersionMismatch {
        /// The skill declaring the dependency.
        skill: String,
        /// The required skill.
        dependency: String,
        /// The constraint.
        required: String,
        /// The installed version.
        found: String,
    },
    /// `requires` forms a cycle. `path` starts and ends with the same skill.
    #[error("dependency cycle: {}", .path.join(" -> "))]
    Cycle {
        /// The cycle, e.g. `["a", "b", "a"]`.
        path: Vec<String>,
    },
}

impl ResolveError {
    /// Stable diagnostic code.
    pub fn code(&self) -> &'static str {
        match self {
            Self::UnknownSkill(_) | Self::MissingDependency { .. } => codes::MISSING_DEPENDENCY,
            Self::VersionMismatch { .. } => codes::VERSION_MISMATCH,
            Self::Cycle { .. } => codes::DEPENDENCY_CYCLE,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mark {
    Visiting,
    Done,
}

/// Topological order of `name` and its transitive `requires` (dependencies first).
///
/// Dependencies are visited in name order, so the result does not depend on declaration order.
pub fn resolve_order(registry: &SkillRegistry, name: &str) -> Result<Vec<String>, ResolveError> {
    if registry.get(name).is_none() {
        return Err(ResolveError::UnknownSkill(name.to_owned()));
    }
    let mut marks = BTreeMap::new();
    let mut stack = Vec::new();
    let mut order = Vec::new();
    visit(registry, name, &mut marks, &mut stack, &mut order)?;
    Ok(order)
}

fn visit(
    registry: &SkillRegistry,
    name: &str,
    marks: &mut BTreeMap<String, Mark>,
    stack: &mut Vec<String>,
    order: &mut Vec<String>,
) -> Result<(), ResolveError> {
    match marks.get(name) {
        Some(Mark::Done) => return Ok(()),
        Some(Mark::Visiting) => {
            let start = stack.iter().position(|s| s == name).expect("on stack");
            let mut path = stack[start..].to_vec();
            path.push(name.to_owned());
            return Err(ResolveError::Cycle { path });
        }
        None => {}
    }
    let skill = registry.get(name).expect("checked by caller");
    marks.insert(name.to_owned(), Mark::Visiting);
    stack.push(name.to_owned());
    let mut deps: Vec<_> = skill.manifest().requires.iter().collect();
    deps.sort_by(|a, b| a.name.cmp(&b.name));
    for dep in deps {
        let Some(target) = registry.get(&dep.name) else {
            return Err(ResolveError::MissingDependency {
                skill: name.to_owned(),
                dependency: dep.name.clone(),
            });
        };
        if !dep.accepts(&target.manifest().version) {
            return Err(ResolveError::VersionMismatch {
                skill: name.to_owned(),
                dependency: dep.name.clone(),
                required: dep.constraint(),
                found: target.manifest().version.to_string(),
            });
        }
        visit(registry, &dep.name, marks, stack, order)?;
    }
    stack.pop();
    marks.insert(name.to_owned(), Mark::Done);
    order.push(name.to_owned());
    Ok(())
}

/// Checks every skill's dependencies and reports each distinct problem once.
pub(crate) fn check_all(registry: &SkillRegistry) -> Vec<Diagnostic> {
    let mut out = BTreeSet::new();
    for skill in registry.iter() {
        for dep in &skill.manifest().requires {
            let error = match registry.get(&dep.name) {
                None => Some(ResolveError::MissingDependency {
                    skill: skill.name().to_owned(),
                    dependency: dep.name.clone(),
                }),
                Some(t) if !dep.accepts(&t.manifest().version) => {
                    Some(ResolveError::VersionMismatch {
                        skill: skill.name().to_owned(),
                        dependency: dep.name.clone(),
                        required: dep.constraint(),
                        found: t.manifest().version.to_string(),
                    })
                }
                Some(_) => None,
            };
            if let Some(e) = error {
                out.insert(Diagnostic::error(
                    e.code(),
                    skill.path(),
                    Some(skill.name()),
                    e.to_string(),
                ));
            }
        }
        if let Err(ResolveError::Cycle { path }) = resolve_order(registry, skill.name()) {
            let cycle = canonical_cycle(&path);
            let first = registry.get(&cycle[0]).expect("cycle member is registered");
            out.insert(Diagnostic::error(
                codes::DEPENDENCY_CYCLE,
                first.path(),
                Some(first.name()),
                ResolveError::Cycle { path: cycle }.to_string(),
            ));
        }
    }
    out.into_iter().collect()
}

/// Rotates a cycle so it starts at its smallest name (`[b, a, b]` → `[a, b, a]`).
fn canonical_cycle(path: &[String]) -> Vec<String> {
    let ring = &path[..path.len() - 1];
    let start = ring
        .iter()
        .enumerate()
        .min_by(|a, b| a.1.cmp(b.1))
        .map_or(0, |(i, _)| i);
    let mut out: Vec<String> = ring[start..]
        .iter()
        .chain(&ring[..start])
        .cloned()
        .collect();
    out.push(out[0].clone());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_cycle_rotates_to_smallest() {
        let s = |v: &[&str]| v.iter().map(|x| (*x).to_owned()).collect::<Vec<_>>();
        assert_eq!(
            canonical_cycle(&s(&["b", "c", "a", "b"])),
            s(&["a", "b", "c", "a"])
        );
        assert_eq!(canonical_cycle(&s(&["a", "a"])), s(&["a", "a"]));
    }
}
