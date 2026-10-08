//! Validation diagnostics with stable codes.

use serde::Serialize;

/// How serious a [`Diagnostic`] is. Only errors fail validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// The skill (or registry) is broken.
    Error,
    /// Suspicious but usable.
    Warning,
}

impl Severity {
    /// Stable name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
        }
    }
}

/// Stable diagnostic codes.
pub mod codes {
    /// Front-matter missing or unterminated, or the file is unreadable.
    pub const FRONT_MATTER: &str = "front_matter";
    /// Front-matter is not valid YAML.
    pub const YAML: &str = "yaml";
    /// Front-matter violates `schemas/skill-manifest.schema.json`.
    pub const SCHEMA: &str = "schema";
    /// `name` differs from the skill directory name.
    pub const NAME_MISMATCH: &str = "name_mismatch";
    /// `version` is not a semantic version.
    pub const INVALID_VERSION: &str = "invalid_version";
    /// A dependency version constraint is not a valid semver requirement.
    pub const INVALID_VERSION_REQ: &str = "invalid_version_req";
    /// A skill lists itself as a dependency, recommendation or route.
    pub const SELF_REFERENCE: &str = "self_reference";
    /// The same skill appears twice in `requires`, or in both `requires` and `optional_recommends`.
    pub const DUPLICATE_DEPENDENCY: &str = "duplicate_dependency";
    /// `invocation` contradicts `user_invocable` / `model_invocable`.
    pub const INVOCATION_CONFLICT: &str = "invocation_conflict";
    /// Neither user- nor model-invocable.
    pub const NOT_INVOCABLE: &str = "not_invocable";
    /// A model-invocable skill or router would cause a user-invoked-only skill to be model-invoked.
    pub const IMPLICIT_MODEL_INVOCATION: &str = "implicit_model_invocation";
    /// A reference path is absolute or escapes the skill directory.
    pub const INVALID_REFERENCE: &str = "invalid_reference";
    /// A declared reference file does not exist.
    pub const MISSING_REFERENCE: &str = "missing_reference";
    /// A trigger normalises to no words, or a keyword is not a single word.
    pub const INVALID_TRIGGER: &str = "invalid_trigger";
    /// An eval case is malformed (e.g. a router case without `skill`).
    pub const INVALID_EVAL_CASE: &str = "invalid_eval_case";
    /// The skill declares no eval cases.
    pub const MISSING_EVAL_CASES: &str = "missing_eval_cases";
    /// Two skills with the same name at the same discovery level.
    pub const DUPLICATE_SKILL: &str = "duplicate_skill";
    /// A lower-level definition is unused because the higher-priority one is broken.
    pub const SHADOWED_BY_BROKEN: &str = "shadowed_by_broken";
    /// A directory in a skills location has no `SKILL.md`.
    pub const MISSING_SKILL_MD: &str = "missing_skill_md";
    /// A skills location exists but cannot be listed.
    pub const INVALID_LOCATION: &str = "invalid_location";
    /// A required skill is not installed.
    pub const MISSING_DEPENDENCY: &str = "missing_dependency";
    /// A required skill is installed at a version outside the constraint.
    pub const VERSION_MISMATCH: &str = "version_mismatch";
    /// `requires` forms a cycle.
    pub const DEPENDENCY_CYCLE: &str = "dependency_cycle";
    /// An `optional_recommends` skill is not installed.
    pub const MISSING_RECOMMENDATION: &str = "missing_recommendation";
    /// A router route target is missing or is itself a router.
    pub const INVALID_ROUTE: &str = "invalid_route";
    /// The body lacks a section required by the skill quality rule.
    pub const MISSING_SECTION: &str = "missing_section";
    /// The body cannot be read.
    pub const UNREADABLE_BODY: &str = "unreadable_body";
}

/// One validation finding.
///
/// Field order defines the sort order used for deterministic output.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct Diagnostic {
    /// Display path of the file concerned (e.g. `.agents/skills/tdd/SKILL.md`).
    pub location: String,
    /// Skill the finding is about, if known.
    pub skill: Option<String>,
    /// Error or warning.
    pub severity: Severity,
    /// Stable code from [`codes`].
    pub code: &'static str,
    /// Human-readable explanation.
    pub message: String,
}

impl Diagnostic {
    /// An error.
    pub fn error(
        code: &'static str,
        location: impl Into<String>,
        skill: Option<&str>,
        message: impl Into<String>,
    ) -> Self {
        Self::new(Severity::Error, code, location, skill, message)
    }

    /// A warning.
    pub fn warning(
        code: &'static str,
        location: impl Into<String>,
        skill: Option<&str>,
        message: impl Into<String>,
    ) -> Self {
        Self::new(Severity::Warning, code, location, skill, message)
    }

    fn new(
        severity: Severity,
        code: &'static str,
        location: impl Into<String>,
        skill: Option<&str>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            location: location.into(),
            skill: skill.map(str::to_owned),
            severity,
            code,
            message: message.into(),
        }
    }

    /// Whether this is an error.
    pub fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }
}
