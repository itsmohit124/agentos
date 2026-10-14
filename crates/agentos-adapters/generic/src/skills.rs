//! Skill discovery for adapters. Bodies stay in the registry until a caller loads them.

use std::path::Path;
use std::sync::Arc;

use agentos_core::fs::RealFs;
use agentos_skills::{Invoker, SkillLoader, builtin};

use crate::error::AdapterError;
use crate::types::{ExposedSkill, SkillExposure};

/// Discovers project, AgentOS, and built-in skills.
///
/// The user home directory is not searched. An adapter must not change its results based on
/// who happens to be logged in.
pub fn expose(project: &Path) -> Result<SkillExposure, AdapterError> {
    let discovery =
        SkillLoader::for_project(project, Arc::new(RealFs), None, builtin::location()).discover();
    let mut skills = Vec::new();
    for skill in discovery.registry.iter() {
        let manifest = skill.manifest();
        skills.push(ExposedSkill {
            name: manifest.name.clone(),
            description: manifest.description.clone(),
            version: manifest.version.to_string(),
            kind: manifest.kind.as_str().to_owned(),
            user_invocable: manifest.invocation.user_invocable,
            model_invocable: manifest.invocation.model_invocable,
            permissions: manifest
                .permissions
                .iter()
                .map(|permission| permission.capability.as_str().to_owned())
                .collect(),
            requires: manifest
                .requires
                .iter()
                .map(|dependency| dependency.name.clone())
                .collect(),
            optional_recommends: manifest.optional_recommends.clone(),
            tools: manifest.tools.clone(),
            level: skill.level().as_str().to_owned(),
            path: skill.path().to_owned(),
        });
    }
    skills.sort_by(|left, right| left.name.cmp(&right.name));
    let mut diagnostics: Vec<String> = discovery
        .diagnostics
        .iter()
        .map(|diagnostic| format!("{} {}", diagnostic.code, diagnostic.message))
        .collect();
    diagnostics.sort();
    Ok(SkillExposure {
        skills,
        diagnostics,
    })
}

/// Returns the skill when `invoker` is allowed to request it.
///
/// This does not load the body and does not run anything.
pub fn authorize(
    project: &Path,
    name: &str,
    invoker: Invoker,
) -> Result<ExposedSkill, AdapterError> {
    let exposure = expose(project)?;
    let Some(skill) = exposure.skills.into_iter().find(|skill| skill.name == name) else {
        return Err(AdapterError::Skills(format!("unknown skill {name:?}")));
    };
    let allowed = match invoker {
        Invoker::User => skill.user_invocable,
        Invoker::Model => skill.model_invocable,
    };
    if !allowed {
        return Err(AdapterError::Skills(format!(
            "skill {name:?} is not {}-invocable",
            match invoker {
                Invoker::User => "user",
                Invoker::Model => "model",
            }
        )));
    }
    Ok(skill)
}

/// Names listed under the model-invocable heading.
pub fn model_section_names(text: &str) -> Vec<String> {
    section_names(text, "## Model-invocable skills")
}

/// Names listed under the user-only heading.
pub fn user_only_section_names(text: &str) -> Vec<String> {
    section_names(text, "## User-invoked only")
}

/// User-only skills that the model section names.
pub fn invocation_violations(text: &str, exposure: &SkillExposure) -> Vec<String> {
    let listed = model_section_names(text);
    exposure
        .skills
        .iter()
        .filter(|skill| !skill.model_invocable && listed.iter().any(|name| name == &skill.name))
        .map(|skill| skill.name.clone())
        .collect()
}

fn section_names(text: &str, heading: &str) -> Vec<String> {
    let Some(start) = text.find(heading) else {
        return Vec::new();
    };
    let rest = &text[start + heading.len()..];
    let end = rest.find("\n## ").unwrap_or(rest.len());
    let mut names = Vec::new();
    for line in rest[..end].lines() {
        let Some(rest) = line.trim().strip_prefix("- `") else {
            continue;
        };
        let Some(name) = rest.split('`').next() else {
            continue;
        };
        if !name.is_empty() {
            names.push(name.to_owned());
        }
    }
    names.sort();
    names
}

/// JSON index of [`SkillExposure`]. HTML comments are not used; they would make the file invalid.
pub fn skills_json(adapter: &str, exposure: &SkillExposure) -> Result<String, AdapterError> {
    #[derive(serde::Serialize)]
    struct File<'a> {
        adapter: &'a str,
        skills: &'a [ExposedSkill],
    }
    let mut text = serde_json::to_string_pretty(&File {
        adapter,
        skills: &exposure.skills,
    })
    .map_err(|err| AdapterError::Skills(err.to_string()))?;
    text.push('\n');
    Ok(text)
}
