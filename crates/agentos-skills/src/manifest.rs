//! The skill manifest (`SKILL.md` front-matter): types, parsing and per-skill validation.
//!
//! Parsing runs in three stages: YAML → JSON Schema (`schemas/skill-manifest.schema.json`) →
//! semantic rules the schema cannot express. All findings of a stage are reported together.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use agentos_permissions::{Capability, PermissionSet};
use serde::{Deserialize, Serialize, Serializer};
use serde_json::Value;

use crate::diagnostic::{Diagnostic, codes};
use crate::schema;
use crate::text::normalize;

/// A skill's semantic version.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SkillVersion(pub semver::Version);

impl fmt::Display for SkillVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl Serialize for SkillVersion {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&self.0)
    }
}

/// A required skill, optionally constrained to a semver range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillDependency {
    /// Name of the required skill.
    pub name: String,
    /// Accepted versions; `None` accepts any version.
    pub version: Option<semver::VersionReq>,
}

impl SkillDependency {
    /// Whether `version` satisfies the constraint.
    pub fn accepts(&self, version: &SkillVersion) -> bool {
        self.version
            .as_ref()
            .is_none_or(|req| req.matches(&version.0))
    }

    /// The constraint as written (normalised by `semver`), or `"*"`.
    pub fn constraint(&self) -> String {
        self.version
            .as_ref()
            .map_or_else(|| "*".to_owned(), ToString::to_string)
    }
}

impl fmt::Display for SkillDependency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.version {
            Some(req) => write!(f, "{} {req}", self.name),
            None => f.write_str(&self.name),
        }
    }
}

impl Serialize for SkillDependency {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Repr<'a> {
            name: &'a str,
            version: Option<String>,
        }
        Repr {
            name: &self.name,
            version: self.version.as_ref().map(ToString::to_string),
        }
        .serialize(serializer)
    }
}

/// One capability a skill declares in `permissions`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct SkillPermission {
    /// The declared capability.
    pub capability: Capability,
}

/// Whether a skill is an ordinary skill or a router over other skills.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SkillKind {
    /// An ordinary skill.
    #[default]
    Skill,
    /// Selects among its `routes` using their declared triggers.
    Router,
}

impl SkillKind {
    /// Stable name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Skill => "skill",
            Self::Router => "router",
        }
    }
}

/// Explicit invocation mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InvocationMode {
    /// Only the human may request it.
    User,
    /// Only the agent may choose it.
    Model,
    /// Both.
    Both,
}

impl InvocationMode {
    /// Stable name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Model => "model",
            Self::Both => "both",
        }
    }
}

/// Who may invoke a skill. User- and model-invocation are independent flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Invocation {
    /// The human may explicitly request the skill.
    pub user_invocable: bool,
    /// The agent may choose the skill autonomously.
    pub model_invocable: bool,
}

impl Invocation {
    /// The equivalent [`InvocationMode`]. `None` if the skill is not invocable at all.
    pub fn mode(self) -> Option<InvocationMode> {
        match (self.user_invocable, self.model_invocable) {
            (true, false) => Some(InvocationMode::User),
            (false, true) => Some(InvocationMode::Model),
            (true, true) => Some(InvocationMode::Both),
            (false, false) => None,
        }
    }
}

/// An input or output of a skill.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkillIo {
    /// Identifier.
    pub name: String,
    /// What it is.
    pub description: String,
    /// Whether it must be provided (inputs) or is always produced (outputs). Default: true.
    #[serde(default = "default_true")]
    pub required: bool,
}

fn default_true() -> bool {
    true
}

/// Deterministic routing rules.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Triggers {
    /// Multi-word phrases matched as contiguous word runs.
    #[serde(default)]
    pub phrases: Vec<String>,
    /// Single words.
    #[serde(default)]
    pub keywords: Vec<String>,
    /// Intent labels matched against labels supplied with the request.
    #[serde(default)]
    pub intents: Vec<String>,
    /// Phrases that disqualify the skill when present.
    #[serde(default)]
    pub exclude: Vec<String>,
}

/// What an eval case expects of the router.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvalExpectation {
    /// The skill ranks first.
    #[default]
    Top,
    /// The skill is ranked.
    Ranked,
    /// The skill is not ranked.
    NotRanked,
}

/// A routing example declared by a skill.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkillEvalCase {
    /// Task text given to the router.
    pub task: String,
    /// Expected outcome.
    #[serde(default)]
    pub expect: EvalExpectation,
    /// Skill the expectation is about; defaults to the declaring skill.
    #[serde(default)]
    pub skill: Option<String>,
    /// Intent labels supplied with the task.
    #[serde(default)]
    pub intents: Vec<String>,
}

/// A validated skill manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SkillManifest {
    /// Skill name (equals its directory name).
    pub name: String,
    /// Concise description used for discovery.
    pub description: String,
    /// Semantic version.
    pub version: SkillVersion,
    /// Skill or router.
    pub kind: SkillKind,
    /// Who may invoke it.
    pub invocation: Invocation,
    /// Required skills (execution dependencies).
    pub requires: Vec<SkillDependency>,
    /// Recommended companion skills (not dependencies).
    pub optional_recommends: Vec<String>,
    /// Router only: skills this router selects among.
    pub routes: Vec<String>,
    /// What it consumes.
    pub inputs: Vec<SkillIo>,
    /// What it produces.
    pub outputs: Vec<SkillIo>,
    /// Abstract tool names it uses.
    pub tools: Vec<String>,
    /// Declared capabilities, sorted. Everything else is denied.
    pub permissions: Vec<SkillPermission>,
    /// Reference files (relative to the skill directory), loaded only on demand.
    pub references: Vec<String>,
    /// Routing rules.
    pub triggers: Triggers,
    /// Routing examples.
    pub eval_cases: Vec<SkillEvalCase>,
    /// Pass-through Agent Skills field.
    pub license: Option<String>,
    /// Pass-through Agent Skills field.
    pub compatibility: Option<String>,
    /// Pass-through Agent Skills field.
    pub metadata: BTreeMap<String, String>,
    /// Pass-through Agent Skills field.
    #[serde(rename = "allowed-tools")]
    pub allowed_tools: Option<String>,
}

impl SkillManifest {
    /// The declared capabilities as a [`PermissionSet`].
    pub fn permission_set(&self) -> PermissionSet {
        self.permissions.iter().map(|p| p.capability).collect()
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawManifest {
    name: String,
    description: String,
    version: String,
    #[serde(default)]
    kind: SkillKind,
    invocation: Option<InvocationMode>,
    user_invocable: Option<bool>,
    model_invocable: Option<bool>,
    #[serde(rename = "disable-model-invocation")]
    disable_model_invocation: Option<bool>,
    #[serde(default)]
    requires: Vec<RawDependency>,
    #[serde(default)]
    optional_recommends: Vec<String>,
    #[serde(default)]
    routes: Vec<String>,
    #[serde(default)]
    inputs: Vec<SkillIo>,
    #[serde(default)]
    outputs: Vec<SkillIo>,
    #[serde(default)]
    tools: Vec<String>,
    #[serde(default)]
    permissions: Vec<Capability>,
    #[serde(default)]
    references: Vec<String>,
    #[serde(default)]
    triggers: Triggers,
    #[serde(default)]
    eval_cases: Vec<SkillEvalCase>,
    license: Option<String>,
    compatibility: Option<String>,
    #[serde(default)]
    metadata: BTreeMap<String, String>,
    #[serde(rename = "allowed-tools")]
    allowed_tools: Option<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RawDependency {
    Name(String),
    Full {
        name: String,
        version: Option<String>,
    },
}

/// Whether this front matter belongs to an AgentOS skill.
///
/// AgentOS skills declare `version`. Host skills that share `.agents/skills/`
/// (Cursor and others) often have only `name` and `description`. Those are not
/// AgentOS packages and must not be rejected or rewritten for omitting `version`.
///
/// Returns an error when the YAML itself cannot be read.
pub fn declares_version(yaml: &str) -> Result<bool, String> {
    let value: Value =
        serde_norway::from_str(yaml).map_err(|err| format!("invalid YAML front-matter: {err}"))?;
    Ok(value
        .as_object()
        .is_some_and(|map| map.contains_key("version")))
}

/// Parses and validates manifest YAML.
///
/// `dir_name`, if given, must equal the manifest `name`. `location` is used in diagnostics.
pub fn parse_manifest(
    yaml: &str,
    dir_name: Option<&str>,
    location: &str,
) -> Result<SkillManifest, Vec<Diagnostic>> {
    let skill_hint = dir_name;
    let value: Value = match serde_norway::from_str(yaml) {
        Ok(v) => v,
        Err(e) => {
            return Err(vec![Diagnostic::error(
                codes::YAML,
                location,
                skill_hint,
                format!("invalid YAML front-matter: {e}"),
            )]);
        }
    };
    let schema_errors = schema::validate(&value);
    if !schema_errors.is_empty() {
        let name = value.get("name").and_then(Value::as_str).or(skill_hint);
        return Err(schema_errors
            .into_iter()
            .map(|(path, msg)| {
                Diagnostic::error(codes::SCHEMA, location, name, format!("{path}: {msg}"))
            })
            .collect());
    }
    let raw: RawManifest = serde_json::from_value(value).map_err(|e| {
        vec![Diagnostic::error(
            codes::SCHEMA,
            location,
            skill_hint,
            e.to_string(),
        )]
    })?;
    build(raw, dir_name, location)
}

fn build(
    mut raw: RawManifest,
    dir_name: Option<&str>,
    location: &str,
) -> Result<SkillManifest, Vec<Diagnostic>> {
    let name = raw.name.clone();
    let mut errors = Vec::new();
    let mut err = |code: &'static str, message: String| {
        errors.push(Diagnostic::error(code, location, Some(&name), message));
    };

    if let Some(dir) = dir_name
        && dir != raw.name
    {
        err(
            codes::NAME_MISMATCH,
            format!(
                "name {:?} must equal the skill directory name {dir:?}",
                raw.name
            ),
        );
    }

    let version = match semver::Version::parse(&raw.version) {
        Ok(v) => Some(SkillVersion(v)),
        Err(e) => {
            err(
                codes::INVALID_VERSION,
                format!(
                    "version {:?} is not a semantic version (e.g. \"0.1.0\"): {e}",
                    raw.version
                ),
            );
            None
        }
    };

    let mut requires = Vec::new();
    let mut seen = BTreeSet::new();
    for dep in std::mem::take(&mut raw.requires) {
        let (dep_name, req) = match dep {
            RawDependency::Name(n) => (n, None),
            RawDependency::Full { name, version } => (name, version),
        };
        if !seen.insert(dep_name.clone()) {
            err(
                codes::DUPLICATE_DEPENDENCY,
                format!("{dep_name:?} is listed more than once in requires"),
            );
            continue;
        }
        let version = match req.map(|r| semver::VersionReq::parse(&r).map_err(|e| (r, e))) {
            None => None,
            Some(Ok(req)) => Some(req),
            Some(Err((r, e))) => {
                err(
                    codes::INVALID_VERSION_REQ,
                    format!("requires {dep_name:?}: {r:?} is not a semver requirement: {e}"),
                );
                None
            }
        };
        requires.push(SkillDependency {
            name: dep_name,
            version,
        });
    }

    for (field, list) in [
        ("requires", seen.iter().cloned().collect::<Vec<_>>()),
        ("optional_recommends", raw.optional_recommends.clone()),
        ("routes", raw.routes.clone()),
    ] {
        if list.contains(&raw.name) {
            err(
                codes::SELF_REFERENCE,
                format!("a skill cannot list itself in {field}"),
            );
        }
    }
    for rec in &raw.optional_recommends {
        if seen.contains(rec) {
            err(
                codes::DUPLICATE_DEPENDENCY,
                format!("{rec:?} is in both requires and optional_recommends"),
            );
        }
    }

    let invocation = resolve_invocation(&raw, &mut err);

    for reference in &raw.references {
        let bad = reference.starts_with('/')
            || reference
                .split('/')
                .any(|part| part.is_empty() || part == "..");
        if bad {
            err(
                codes::INVALID_REFERENCE,
                format!(
                    "reference {reference:?} must be a relative path inside the skill directory"
                ),
            );
        }
    }

    for phrase in raw
        .triggers
        .phrases
        .iter()
        .chain(&raw.triggers.exclude)
        .chain(&raw.triggers.intents)
    {
        if normalize(phrase).is_empty() {
            err(
                codes::INVALID_TRIGGER,
                format!("trigger {phrase:?} contains no words"),
            );
        }
    }
    for keyword in &raw.triggers.keywords {
        if normalize(keyword).len() != 1 {
            err(
                codes::INVALID_TRIGGER,
                format!("keyword {keyword:?} must be a single word; use phrases for word runs"),
            );
        }
    }

    for (i, case) in raw.eval_cases.iter().enumerate() {
        if raw.kind == SkillKind::Router && case.skill.is_none() {
            err(
                codes::INVALID_EVAL_CASE,
                format!(
                    "eval case {} of a router must name the expected `skill`",
                    i + 1
                ),
            );
        }
    }

    if !errors.is_empty() {
        return Err(errors);
    }
    let mut permissions: Vec<SkillPermission> = raw
        .permissions
        .into_iter()
        .map(|capability| SkillPermission { capability })
        .collect();
    permissions.sort();
    Ok(SkillManifest {
        name: raw.name,
        description: raw.description,
        version: version.expect("checked above"),
        kind: raw.kind,
        invocation,
        requires,
        optional_recommends: raw.optional_recommends,
        routes: raw.routes,
        inputs: raw.inputs,
        outputs: raw.outputs,
        tools: raw.tools,
        permissions,
        references: raw.references,
        triggers: raw.triggers,
        eval_cases: raw.eval_cases,
        license: raw.license,
        compatibility: raw.compatibility,
        metadata: raw.metadata,
        allowed_tools: raw.allowed_tools,
    })
}

/// Resolves invocation flags. Model invocation is only ever enabled by an explicit
/// `model_invocable: true` or `invocation: model|both`; it is never a default or a fallback.
fn resolve_invocation(raw: &RawManifest, err: &mut impl FnMut(&'static str, String)) -> Invocation {
    let invocation = match raw.invocation {
        Some(mode) => {
            let (user, model) = match mode {
                InvocationMode::User => (true, false),
                InvocationMode::Model => (false, true),
                InvocationMode::Both => (true, true),
            };
            for (field, explicit, implied) in [
                ("user_invocable", raw.user_invocable, user),
                ("model_invocable", raw.model_invocable, model),
            ] {
                if explicit.is_some_and(|v| v != implied) {
                    err(
                        codes::INVOCATION_CONFLICT,
                        format!(
                            "invocation: {} contradicts {field}: {}",
                            mode.as_str(),
                            !implied
                        ),
                    );
                }
            }
            Invocation {
                user_invocable: user,
                model_invocable: model,
            }
        }
        None => Invocation {
            user_invocable: raw.user_invocable.unwrap_or(true),
            model_invocable: raw.model_invocable.unwrap_or(false),
        },
    };
    if invocation.mode().is_none() {
        err(
            codes::NOT_INVOCABLE,
            "skill is neither user- nor model-invocable (model invocation is never enabled implicitly; set model_invocable: true explicitly)".to_owned(),
        );
    }
    if let Some(disabled) = raw.disable_model_invocation {
        let host_allows_model = !disabled;
        if host_allows_model != invocation.model_invocable {
            err(
                codes::INVOCATION_CONFLICT,
                "disable-model-invocation contradicts model invocation; it cannot widen a user-only skill".to_owned(),
            );
        }
    }
    invocation
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: &str = "name: demo\ndescription: A demo skill.\nversion: \"0.1.0\"\n";

    fn parse(yaml: &str) -> Result<SkillManifest, Vec<Diagnostic>> {
        parse_manifest(yaml, Some("demo"), "demo/SKILL.md")
    }

    fn codes_of(yaml: &str) -> Vec<&'static str> {
        let mut c: Vec<_> = parse(yaml).unwrap_err().iter().map(|d| d.code).collect();
        c.sort_unstable();
        c.dedup();
        c
    }

    #[test]
    fn a_host_skill_omits_version() {
        assert!(!declares_version("name: codebase-design\ndescription: Shared.\n").unwrap());
        assert!(declares_version(MIN).unwrap());
        assert!(declares_version("version: [\n").is_err());
    }

    #[test]
    fn minimal_manifest_has_safe_defaults() {
        let m = parse(MIN).unwrap();
        assert_eq!(m.name, "demo");
        assert_eq!(m.version.to_string(), "0.1.0");
        assert_eq!(m.kind, SkillKind::Skill);
        assert_eq!(
            m.invocation,
            Invocation {
                user_invocable: true,
                model_invocable: false
            }
        );
        assert!(m.permissions.is_empty());
        assert!(m.permission_set().is_empty());
        assert!(m.requires.is_empty());
    }

    #[test]
    fn full_manifest_parses_every_field() {
        let yaml = format!(
            "{MIN}kind: skill\ninvocation: both\nrequires:\n  - base\n  - name: other\n    version: \"^1.2\"\n\
             optional_recommends: [extra]\ninputs:\n  - name: task\n    description: The task.\n\
             outputs:\n  - name: plan\n    description: A plan.\n    required: false\n\
             tools: [test-runner, vcs]\npermissions: [shell, fs.read]\nreferences: [references/a.md]\n\
             triggers:\n  phrases: [write tests first]\n  keywords: [test]\n  intents: [testing]\n  exclude: [no tests]\n\
             eval_cases:\n  - task: write tests first\n  - task: deploy\n    expect: not_ranked\n\
             license: MIT\ncompatibility: any\nmetadata:\n  owner: team\nallowed-tools: Read\n"
        );
        let m = parse(&yaml).unwrap();
        assert!(m.invocation.user_invocable && m.invocation.model_invocable);
        assert_eq!(m.requires[0].to_string(), "base");
        assert_eq!(m.requires[1].to_string(), "other ^1.2");
        assert_eq!(m.optional_recommends, ["extra"]);
        assert!(!m.outputs[0].required && m.inputs[0].required);
        assert_eq!(
            m.permissions
                .iter()
                .map(|p| p.capability.as_str())
                .collect::<Vec<_>>(),
            ["fs.read", "shell"]
        );
        assert_eq!(m.eval_cases[1].expect, EvalExpectation::NotRanked);
        assert_eq!(m.metadata["owner"], "team");
        assert_eq!(m.allowed_tools.as_deref(), Some("Read"));
    }

    #[test]
    fn required_fields_are_enforced_by_the_schema() {
        assert_eq!(
            codes_of("name: demo\ndescription: d\n"),
            [codes::SCHEMA],
            "missing version"
        );
        assert_eq!(
            codes_of("name: demo\nversion: \"1.0.0\"\n"),
            [codes::SCHEMA]
        );
        assert_eq!(
            codes_of("description: d\nversion: \"1.0.0\"\n"),
            [codes::SCHEMA]
        );
        assert_eq!(codes_of(""), [codes::SCHEMA]);
    }

    #[test]
    fn unknown_fields_and_bad_types_are_schema_errors() {
        assert_eq!(codes_of(&format!("{MIN}requries: [x]\n")), [codes::SCHEMA]);
        assert_eq!(
            codes_of("name: demo\ndescription: d\nversion: 1.0\n"),
            [codes::SCHEMA]
        );
        assert_eq!(
            codes_of(&format!("{MIN}permissions: [root]\n")),
            [codes::SCHEMA]
        );
        assert_eq!(
            codes_of(&format!("{MIN}user_invocable: yes please\n")),
            [codes::SCHEMA]
        );
    }

    #[test]
    fn invalid_yaml_is_reported() {
        assert_eq!(codes_of("name: [unclosed\n"), [codes::YAML]);
    }

    #[test]
    fn version_handling() {
        assert_eq!(
            codes_of("name: demo\ndescription: d\nversion: \"1.0\"\n"),
            [codes::INVALID_VERSION]
        );
        let m = parse("name: demo\ndescription: d\nversion: \"1.2.3-beta.1\"\n").unwrap();
        assert_eq!(m.version.to_string(), "1.2.3-beta.1");
        assert_eq!(
            codes_of(&format!(
                "{MIN}requires:\n  - name: base\n    version: \"not a range\"\n"
            )),
            [codes::INVALID_VERSION_REQ]
        );
        let m = parse(&format!(
            "{MIN}requires:\n  - name: base\n    version: \">=1.2, <2\"\n"
        ))
        .unwrap();
        let dep = &m.requires[0];
        let v = |s: &str| SkillVersion(semver::Version::parse(s).unwrap());
        assert!(dep.accepts(&v("1.5.0")));
        assert!(!dep.accepts(&v("2.0.0")));
        assert!(!dep.accepts(&v("1.1.9")));
        assert!(
            SkillDependency {
                name: "x".into(),
                version: None
            }
            .accepts(&v("9.9.9"))
        );
    }

    #[test]
    fn name_must_match_directory() {
        let errs = parse_manifest(MIN, Some("other"), "other/SKILL.md").unwrap_err();
        assert_eq!(errs[0].code, codes::NAME_MISMATCH);
        assert!(parse_manifest(MIN, None, "x").is_ok());
    }

    #[test]
    fn self_and_duplicate_dependencies_are_rejected() {
        assert_eq!(
            codes_of(&format!("{MIN}requires: [demo]\n")),
            [codes::SELF_REFERENCE]
        );
        assert_eq!(
            codes_of(&format!("{MIN}requires:\n  - a\n  - name: a\n")),
            [codes::DUPLICATE_DEPENDENCY]
        );
        assert_eq!(
            codes_of(&format!("{MIN}requires: [a]\noptional_recommends: [a]\n")),
            [codes::DUPLICATE_DEPENDENCY]
        );
    }

    #[test]
    fn invocation_flags_are_independent_and_explicit() {
        let inv = |extra: &str| parse(&format!("{MIN}{extra}")).unwrap().invocation;
        assert_eq!(
            inv("model_invocable: true\n"),
            Invocation {
                user_invocable: true,
                model_invocable: true
            }
        );
        assert_eq!(
            inv("invocation: model\n"),
            Invocation {
                user_invocable: false,
                model_invocable: true
            }
        );
        assert_eq!(
            inv("invocation: user\nuser_invocable: true\nmodel_invocable: false\n").mode(),
            Some(InvocationMode::User)
        );
    }

    #[test]
    fn user_invoked_skill_is_never_made_model_invocable_implicitly() {
        // Turning off user invocation does not fall back to model invocation.
        assert_eq!(
            codes_of(&format!("{MIN}user_invocable: false\n")),
            [codes::NOT_INVOCABLE]
        );
        // An explicit user-only mode cannot be widened by a contradicting flag.
        assert!(
            !parse(&format!(
                "{MIN}invocation: user\ndisable-model-invocation: true\n"
            ))
            .unwrap()
            .invocation
            .model_invocable
        );
        assert_eq!(
            codes_of(&format!(
                "{MIN}invocation: user\ndisable-model-invocation: false\n"
            )),
            [codes::INVOCATION_CONFLICT]
        );
        assert_eq!(
            codes_of(&format!(
                "{MIN}invocation: both\ndisable-model-invocation: true\n"
            )),
            [codes::INVOCATION_CONFLICT]
        );
        assert_eq!(
            codes_of(&format!("{MIN}invocation: user\nmodel_invocable: true\n")),
            [codes::INVOCATION_CONFLICT]
        );
        assert_eq!(
            codes_of(&format!("{MIN}invocation: both\nmodel_invocable: false\n")),
            [codes::INVOCATION_CONFLICT]
        );
        // The default for a skill that only says it is user-invocable stays user-only.
        assert!(
            !parse(&format!("{MIN}user_invocable: true\n"))
                .unwrap()
                .invocation
                .model_invocable
        );
    }

    #[test]
    fn references_must_stay_inside_the_skill() {
        assert_eq!(
            codes_of(&format!("{MIN}references: [../secrets.md]\n")),
            [codes::INVALID_REFERENCE]
        );
        assert_eq!(
            codes_of(&format!("{MIN}references: [a//b.md]\n")),
            [codes::INVALID_REFERENCE]
        );
        assert_eq!(
            codes_of(&format!("{MIN}references: [/etc/passwd]\n")),
            [codes::SCHEMA]
        );
    }

    #[test]
    fn triggers_are_validated() {
        assert_eq!(
            codes_of(&format!("{MIN}triggers:\n  phrases: [\"!!\"]\n")),
            [codes::INVALID_TRIGGER]
        );
        assert_eq!(
            codes_of(&format!("{MIN}triggers:\n  keywords: [test-driven]\n")),
            [codes::INVALID_TRIGGER]
        );
        assert_eq!(
            codes_of(&format!("{MIN}triggers:\n  keywords: [two words]\n")),
            [codes::SCHEMA]
        );
    }

    #[test]
    fn router_eval_cases_must_name_a_skill() {
        let router = "name: demo\ndescription: d\nversion: \"0.1.0\"\nkind: router\nroutes: [a]\n";
        assert!(parse(router).is_ok());
        assert_eq!(
            codes_of(&format!("{router}eval_cases:\n  - task: x\n")),
            [codes::INVALID_EVAL_CASE]
        );
        assert!(parse(&format!("{router}eval_cases:\n  - task: x\n    skill: a\n")).is_ok());
    }

    #[test]
    fn all_semantic_errors_are_reported_together() {
        let errs = parse_manifest(
            "name: demo\ndescription: d\nversion: \"x\"\nrequires: [demo]\nuser_invocable: false\n",
            Some("other"),
            "loc",
        )
        .unwrap_err();
        let mut c: Vec<_> = errs.iter().map(|d| d.code).collect();
        c.sort_unstable();
        assert_eq!(
            c,
            [
                codes::INVALID_VERSION,
                codes::NAME_MISMATCH,
                codes::NOT_INVOCABLE,
                codes::SELF_REFERENCE
            ]
        );
        assert!(errs.iter().all(|d| d.skill.as_deref() == Some("demo")));
    }

    #[test]
    fn a_manifest_from_before_disable_model_invocation_still_parses() {
        let previous = "name: demo\ndescription: A skill written before the host control existed.\nversion: \"1.2.0\"\n";
        let manifest = parse_manifest(previous, Some("demo"), "previous").unwrap();
        assert_eq!(manifest.version.to_string(), "1.2.0");
        assert!(!manifest.invocation.model_invocable);
        let accepted = SkillDependency {
            name: "demo".to_owned(),
            version: Some("^1.0".parse().unwrap()),
        };
        assert!(accepted.accepts(&manifest.version));
        let next_major = SkillVersion(semver::Version::new(2, 0, 0));
        assert!(!accepted.accepts(&next_major));
    }
}
