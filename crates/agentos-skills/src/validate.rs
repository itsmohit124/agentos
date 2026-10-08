//! Full validation: discovery problems, registry-level rules and skill bodies.

use serde::Serialize;

use crate::diagnostic::{Diagnostic, Severity, codes};
use crate::loader::{Discovery, Level};
use crate::manifest::SkillKind;
use crate::registry::{Skill, SkillRegistry};
use crate::resolve;

/// Body sections required by the skill quality rule, in the order they should appear.
///
/// Each is a `## <title>` heading (case-insensitive). Missing sections are errors for built-in
/// skills and warnings elsewhere.
pub const REQUIRED_SECTIONS: [&str; 6] = [
    "When to use",
    "Inputs",
    "Procedure",
    "Outputs",
    "Must not",
    "Validation",
];

/// Outcome of [`validate`].
#[derive(Debug, Clone, Serialize)]
pub struct ValidationReport {
    /// Number of skills that loaded and were checked further.
    pub skills_checked: usize,
    /// Every finding, sorted.
    pub diagnostics: Vec<Diagnostic>,
}

impl ValidationReport {
    /// Number of errors.
    pub fn errors(&self) -> usize {
        self.diagnostics.iter().filter(|d| d.is_error()).count()
    }

    /// Number of warnings.
    pub fn warnings(&self) -> usize {
        self.diagnostics.len() - self.errors()
    }

    /// Whether there are no errors.
    pub fn is_ok(&self) -> bool {
        self.errors() == 0
    }
}

/// Validates everything discovered, loading every body (unlike discovery).
pub fn validate(discovery: &Discovery) -> ValidationReport {
    let registry = &discovery.registry;
    let mut diagnostics = discovery.diagnostics.clone();
    diagnostics.extend(resolve::check_all(registry));
    for skill in registry.iter() {
        check_links(registry, skill, &mut diagnostics);
        check_body(skill, &mut diagnostics);
    }
    diagnostics.sort();
    diagnostics.dedup();
    ValidationReport {
        skills_checked: registry.len(),
        diagnostics,
    }
}

fn check_links(registry: &SkillRegistry, skill: &Skill, out: &mut Vec<Diagnostic>) {
    let m = skill.manifest();
    let err =
        |code, message: String| Diagnostic::error(code, skill.path(), Some(skill.name()), message);

    for rec in &m.optional_recommends {
        if registry.get(rec).is_none() {
            out.push(Diagnostic::warning(
                codes::MISSING_RECOMMENDATION,
                skill.path(),
                Some(skill.name()),
                format!("recommended skill {rec:?} is not installed"),
            ));
        }
    }

    if m.invocation.model_invocable {
        for dep in &m.requires {
            if let Some(target) = registry.get(&dep.name)
                && !target.manifest().invocation.model_invocable
            {
                out.push(err(
                    codes::IMPLICIT_MODEL_INVOCATION,
                    format!(
                        "model-invocable skill requires user-invoked-only skill {:?}; the model would invoke it implicitly",
                        dep.name
                    ),
                ));
            }
        }
    }

    for route in &m.routes {
        match registry.get(route) {
            None => out.push(err(
                codes::INVALID_ROUTE,
                format!("route target {route:?} is not installed"),
            )),
            Some(t) if t.manifest().kind == SkillKind::Router => out.push(err(
                codes::INVALID_ROUTE,
                format!("route target {route:?} is itself a router"),
            )),
            Some(t) if m.invocation.model_invocable && !t.manifest().invocation.model_invocable => {
                out.push(err(
                    codes::IMPLICIT_MODEL_INVOCATION,
                    format!("model-invocable router routes to user-invoked-only skill {route:?}"),
                ));
            }
            Some(_) => {}
        }
    }

    for (i, case) in m.eval_cases.iter().enumerate() {
        if let Some(target) = &case.skill
            && registry.get(target).is_none()
        {
            out.push(err(
                codes::INVALID_EVAL_CASE,
                format!("eval case {} names unknown skill {target:?}", i + 1),
            ));
        }
    }
}

fn check_body(skill: &Skill, out: &mut Vec<Diagnostic>) {
    let body = match skill.load_body() {
        Ok(body) => body,
        Err(e) => {
            out.push(Diagnostic::error(
                codes::UNREADABLE_BODY,
                skill.path(),
                Some(skill.name()),
                e.to_string(),
            ));
            return;
        }
    };
    let severity = if skill.level() == Level::Builtin {
        Severity::Error
    } else {
        Severity::Warning
    };
    for missing in missing_sections(&body) {
        let message = format!("body has no \"## {missing}\" section (skill quality rule)");
        out.push(match severity {
            Severity::Error => Diagnostic::error(
                codes::MISSING_SECTION,
                skill.path(),
                Some(skill.name()),
                message,
            ),
            Severity::Warning => Diagnostic::warning(
                codes::MISSING_SECTION,
                skill.path(),
                Some(skill.name()),
                message,
            ),
        });
    }
}

/// Required sections missing from `body`. A `Procedure` section without a numbered step counts
/// as missing, because the procedure must be ordered.
pub fn missing_sections(body: &str) -> Vec<&'static str> {
    let mut current: Option<String> = None;
    let mut found: Vec<String> = Vec::new();
    let mut procedure_numbered = false;
    for line in body.lines() {
        if let Some(title) = line.strip_prefix("## ") {
            let title = title.trim().to_lowercase();
            found.push(title.clone());
            current = Some(title);
        } else if current.as_deref() == Some("procedure") && is_numbered(line) {
            procedure_numbered = true;
        }
    }
    REQUIRED_SECTIONS
        .into_iter()
        .filter(|s| {
            let s_lower = s.to_lowercase();
            !found.contains(&s_lower) || (s_lower == "procedure" && !procedure_numbered)
        })
        .collect()
}

fn is_numbered(line: &str) -> bool {
    let t = line.trim_start();
    let digits = t.chars().take_while(char::is_ascii_digit).count();
    digits > 0 && t[digits..].starts_with(". ")
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = "## When to use\nx\n## Inputs\nx\n## Procedure\n1. a\n2. b\n## Outputs\nx\n## Must not\nx\n## Validation\nx\n";

    #[test]
    fn complete_body_has_no_missing_sections() {
        assert!(missing_sections(GOOD).is_empty());
        assert!(missing_sections(&GOOD.to_uppercase()).is_empty());
    }

    #[test]
    fn missing_and_unordered_sections_are_reported() {
        assert_eq!(missing_sections(""), REQUIRED_SECTIONS.to_vec());
        let no_steps = GOOD.replace("1. a\n2. b\n", "- a\n");
        assert_eq!(missing_sections(&no_steps), ["Procedure"]);
        let no_must_not = GOOD.replace("## Must not\n", "");
        assert_eq!(missing_sections(&no_must_not), ["Must not"]);
    }
}
