//! Skill-system evaluation (`evals/skills/*.json`).
//!
//! Case kinds (field `kind`):
//!
//! | kind | oracle |
//! |---|---|
//! | `manifest` | a single skill directory loads (or fails) with exactly the expected diagnostic codes |
//! | `discovery` | winning level per skill, overrides, and the exact set of diagnostic codes |
//! | `resolve` | exact dependency order, or the expected error code / cycle path |
//! | `route` | expected top skill (or none), skills that must not be ranked, or a routing error |
//! | `permission` | `check(skill, capability)` verdict |
//! | `validate` | full validation outcome and diagnostic codes |
//! | `skill_eval` | every skill's own `eval_cases` hold (expanded to one result per eval case) |
//!
//! Every case names a `fixture`: a directory under `fixtures/` (with optional `project/`, `user/`,
//! `builtin/` subdirectories), or `builtin:skills` for the skills shipped with AgentOS.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use agentos_core::fs::{FileSystem, RealFs};
use agentos_permissions::Capability;
use agentos_skills::{
    AGENTOS_SKILLS_DIR, Discovery, EvalExpectation, Invoker, Level, PROJECT_SKILLS_DIR, RouteError,
    RouteRequest, SkillKind, SkillLoader, SkillLocation, SkillRouter, validate,
};
use serde::{Deserialize, Deserializer};

use crate::{CaseResult, EvalError, EvalReport};

/// Fixture name that selects the built-in skills.
pub const BUILTIN_FIXTURE: &str = "builtin:skills";

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Case {
    Manifest(ManifestCase),
    Discovery(DiscoveryCase),
    Resolve(ResolveCase),
    Route(RouteCase),
    Permission(PermissionCase),
    Validate(ValidateCase),
    SkillEval(SkillEvalCase),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestCase {
    id: String,
    description: String,
    fixture: String,
    oracle: String,
    expect_valid: bool,
    expect_codes: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DiscoveryCase {
    id: String,
    description: String,
    fixture: String,
    oracle: String,
    expect_skills: BTreeMap<String, Level>,
    #[serde(default)]
    expect_overrides: BTreeMap<String, Vec<Level>>,
    expect_codes: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ResolveCase {
    id: String,
    description: String,
    fixture: String,
    oracle: String,
    target: String,
    #[serde(default)]
    expect_order: Option<Vec<String>>,
    #[serde(default)]
    expect_error: Option<String>,
    #[serde(default)]
    expect_cycle: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RouteCase {
    id: String,
    description: String,
    fixture: String,
    oracle: String,
    task: String,
    invoker: InvokerName,
    #[serde(default)]
    intents: Vec<String>,
    #[serde(default)]
    router: Option<String>,
    #[serde(default, deserialize_with = "present")]
    expect_top: Option<Option<String>>,
    #[serde(default)]
    must_not_rank: Vec<String>,
    #[serde(default)]
    expect_error: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
enum InvokerName {
    User,
    Model,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PermissionCase {
    id: String,
    description: String,
    fixture: String,
    oracle: String,
    skill: String,
    capability: String,
    expect: Verdict,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Verdict {
    Allow,
    Deny,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ValidateCase {
    id: String,
    description: String,
    fixture: String,
    oracle: String,
    expect_ok: bool,
    expect_codes: Vec<String>,
    #[serde(default)]
    expect_skills: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SkillEvalCase {
    id: String,
    description: String,
    fixture: String,
    oracle: String,
}

/// Distinguishes a missing field (`None`) from an explicit `null` (`Some(None)`).
fn present<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Option<String>>, D::Error> {
    Option::<String>::deserialize(d).map(Some)
}

/// Runs every case in `cases_dir`. `fixtures_dir` is the workspace `fixtures/` directory and
/// `builtin` the built-in skills location (used by `builtin:skills` cases).
pub fn run(
    cases_dir: &Path,
    fixtures_dir: &Path,
    builtin: SkillLocation,
) -> Result<EvalReport, EvalError> {
    let fs = RealFs;
    if !fs.is_dir(cases_dir) {
        return Err(EvalError::NoCases(cases_dir.to_path_buf()));
    }
    let files: Vec<PathBuf> = fs
        .read_dir(cases_dir)?
        .into_iter()
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    if files.is_empty() {
        return Err(EvalError::NoCases(cases_dir.to_path_buf()));
    }
    let ctx = Ctx {
        fixtures_dir: fixtures_dir.to_path_buf(),
        builtin,
    };
    let mut cases = Vec::new();
    let mut ids = BTreeSet::new();
    for path in files {
        let text = fs.read_to_string(&path)?;
        let invalid = |message: String| EvalError::InvalidCase {
            path: path.clone(),
            message,
        };
        let case: Case = serde_json::from_str(&text).map_err(|e| invalid(e.to_string()))?;
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let (id, oracle) = case.identity();
        if id != stem {
            return Err(invalid(format!(
                "id {id:?} must equal the file name {stem:?}"
            )));
        }
        if oracle.trim().is_empty() {
            return Err(invalid("oracle must not be empty".to_owned()));
        }
        if !ids.insert(id.to_owned()) {
            return Err(invalid(format!("duplicate id {id:?}")));
        }
        cases.extend(ctx.run(&case));
    }
    Ok(EvalReport {
        area: "skills".to_owned(),
        cases_dir: cases_dir.to_path_buf(),
        cases,
    })
}

impl Case {
    fn identity(&self) -> (&str, &str) {
        match self {
            Self::Manifest(c) => (&c.id, &c.oracle),
            Self::Discovery(c) => (&c.id, &c.oracle),
            Self::Resolve(c) => (&c.id, &c.oracle),
            Self::Route(c) => (&c.id, &c.oracle),
            Self::Permission(c) => (&c.id, &c.oracle),
            Self::Validate(c) => (&c.id, &c.oracle),
            Self::SkillEval(c) => (&c.id, &c.oracle),
        }
    }
}

struct Ctx {
    fixtures_dir: PathBuf,
    builtin: SkillLocation,
}

fn result(id: &str, kind: &str, description: &str, passed: bool, detail: String) -> CaseResult {
    CaseResult {
        id: id.to_owned(),
        kind: kind.to_owned(),
        description: description.to_owned(),
        passed,
        detail,
    }
}

fn codes(discovery_or_report: impl IntoIterator<Item = &'static str>) -> Vec<String> {
    let set: BTreeSet<&str> = discovery_or_report.into_iter().collect();
    set.into_iter().map(str::to_owned).collect()
}

fn sorted(mut v: Vec<String>) -> Vec<String> {
    v.sort();
    v.dedup();
    v
}

impl Ctx {
    fn discover(&self, fixture: &str) -> Discovery {
        if fixture == BUILTIN_FIXTURE {
            return SkillLoader::new()
                .with_location(self.builtin.clone())
                .discover();
        }
        let fs: Arc<dyn FileSystem> = Arc::new(RealFs);
        let base = self.fixtures_dir.join(fixture);
        let project = base.join("project");
        let mut loader = SkillLoader::new()
            .with_location(
                SkillLocation::new(Level::Project, project.join(PROJECT_SKILLS_DIR), fs.clone())
                    .with_label(PROJECT_SKILLS_DIR),
            )
            .with_location(
                SkillLocation::new(Level::Agentos, project.join(AGENTOS_SKILLS_DIR), fs.clone())
                    .with_label(AGENTOS_SKILLS_DIR),
            )
            .with_location(
                SkillLocation::new(Level::User, base.join("user"), fs.clone()).with_label("user"),
            )
            .with_location(
                SkillLocation::new(Level::Builtin, base.join("builtin"), fs).with_label("builtin:"),
            );
        if !project.exists() && !base.join("user").exists() && !base.join("builtin").exists() {
            // A bare directory of skills (manifest cases).
            loader = SkillLoader::new().with_location(
                SkillLocation::new(Level::Project, base, Arc::new(RealFs)).with_label(fixture),
            );
        }
        loader.discover()
    }

    fn run(&self, case: &Case) -> Vec<CaseResult> {
        match case {
            Case::Manifest(c) => vec![self.manifest(c)],
            Case::Discovery(c) => vec![self.discovery(c)],
            Case::Resolve(c) => vec![self.resolve(c)],
            Case::Route(c) => vec![self.route(c)],
            Case::Permission(c) => vec![self.permission(c)],
            Case::Validate(c) => vec![self.validate(c)],
            Case::SkillEval(c) => self.skill_eval(c),
        }
    }

    fn manifest(&self, c: &ManifestCase) -> CaseResult {
        let d = self.discover(&c.fixture);
        let valid = !d.registry.is_empty() && d.diagnostics.iter().all(|x| !x.is_error());
        let got = codes(d.diagnostics.iter().map(|x| x.code));
        let passed = valid == c.expect_valid && got == sorted(c.expect_codes.clone());
        let messages: Vec<String> = d
            .diagnostics
            .iter()
            .map(|x| format!("{}: {}", x.code, x.message))
            .collect();
        result(
            &c.id,
            "manifest",
            &c.description,
            passed,
            format!(
                "valid={valid} codes={got:?}{}{}",
                if passed {
                    String::new()
                } else {
                    format!(
                        " (expected valid={} codes={:?})",
                        c.expect_valid,
                        sorted(c.expect_codes.clone())
                    )
                },
                if messages.is_empty() {
                    String::new()
                } else {
                    format!(" | {}", messages.join("; "))
                }
            ),
        )
    }

    fn discovery(&self, c: &DiscoveryCase) -> CaseResult {
        let d = self.discover(&c.fixture);
        let skills: BTreeMap<String, Level> = d
            .registry
            .iter()
            .map(|s| (s.name().to_owned(), s.level()))
            .collect();
        let overrides: BTreeMap<String, Vec<Level>> = d
            .registry
            .iter()
            .filter(|s| !s.overrides().is_empty())
            .map(|s| {
                (
                    s.name().to_owned(),
                    s.overrides().iter().map(|o| o.level).collect(),
                )
            })
            .collect();
        let got = codes(d.diagnostics.iter().map(|x| x.code));
        let want = sorted(c.expect_codes.clone());
        let passed = skills == c.expect_skills && overrides == c.expect_overrides && got == want;
        let mut detail = format!("skills={skills:?} overrides={overrides:?} codes={got:?}");
        if !passed {
            detail.push_str(&format!(
                " (expected skills={:?} overrides={:?} codes={want:?})",
                c.expect_skills, c.expect_overrides
            ));
        }
        result(&c.id, "discovery", &c.description, passed, detail)
    }

    fn resolve(&self, c: &ResolveCase) -> CaseResult {
        let d = self.discover(&c.fixture);
        let outcome = d.registry.resolve(&c.target).map(|order| {
            order
                .iter()
                .map(|s| s.name().to_owned())
                .collect::<Vec<_>>()
        });
        let (passed, detail) = match (&outcome, &c.expect_order, &c.expect_error) {
            (Ok(order), Some(want), None) => (
                order == want,
                format!("order={order:?} (expected {want:?})"),
            ),
            (Err(e), None, Some(code)) => {
                let cycle_ok = match (&c.expect_cycle, e) {
                    (None, _) => true,
                    (Some(want), agentos_skills::ResolveError::Cycle { path }) => path == want,
                    (Some(_), _) => false,
                };
                (
                    e.code() == code && cycle_ok,
                    format!(
                        "error {}: {e} (expected {code}{})",
                        e.code(),
                        c.expect_cycle
                            .as_ref()
                            .map(|p| format!(", cycle {}", p.join(" -> ")))
                            .unwrap_or_default()
                    ),
                )
            }
            (Ok(order), _, _) => (false, format!("order={order:?}, expected an error")),
            (Err(e), _, _) => (false, format!("unexpected error {}: {e}", e.code())),
        };
        result(&c.id, "resolve", &c.description, passed, detail)
    }

    fn route(&self, c: &RouteCase) -> CaseResult {
        let d = self.discover(&c.fixture);
        let request = RouteRequest {
            task: c.task.clone(),
            intents: c.intents.clone(),
            invoker: match c.invoker {
                InvokerName::User => Invoker::User,
                InvokerName::Model => Invoker::Model,
            },
            router: c.router.clone(),
        };
        let outcome = SkillRouter.route(&d.registry, &request);
        let (passed, detail) = match (&outcome, &c.expect_error) {
            (Err(e), Some(code)) => (
                route_error_code(e) == code,
                format!("error {}: {e} (expected {code})", route_error_code(e)),
            ),
            (Err(e), None) => (false, format!("unexpected error: {e}")),
            (Ok(_), Some(code)) => (false, format!("routed, expected error {code}")),
            (Ok(r), None) => {
                let top_ok = match &c.expect_top {
                    None => true,
                    Some(want) => r.top() == want.as_deref(),
                };
                let ranked: Vec<&str> = r.ranked.iter().map(|x| x.name.as_str()).collect();
                let leaked: Vec<&String> = c
                    .must_not_rank
                    .iter()
                    .filter(|n| ranked.contains(&n.as_str()))
                    .collect();
                let mut detail = match r.ranked.first() {
                    Some(top) => format!(
                        "top={} score={} [{}]",
                        top.name,
                        top.score,
                        top.reasons.join(", ")
                    ),
                    None => "top=none".to_owned(),
                };
                if r.ranked.len() > 1 {
                    let rest: Vec<String> = r.ranked[1..]
                        .iter()
                        .map(|x| format!("{}:{}", x.name, x.score))
                        .collect();
                    detail.push_str(&format!(" others=[{}]", rest.join(", ")));
                }
                if !r.rejected.is_empty() {
                    let rej: Vec<String> = r
                        .rejected
                        .iter()
                        .map(|x| format!("{} ({})", x.name, x.reason))
                        .collect();
                    detail.push_str(&format!(" rejected=[{}]", rej.join(", ")));
                }
                if !top_ok {
                    detail.push_str(&format!(
                        " (expected top={:?})",
                        c.expect_top.clone().flatten()
                    ));
                }
                if !leaked.is_empty() {
                    detail.push_str(&format!(" (must not rank {leaked:?})"));
                }
                (top_ok && leaked.is_empty(), detail)
            }
        };
        result(&c.id, "route", &c.description, passed, detail)
    }

    fn permission(&self, c: &PermissionCase) -> CaseResult {
        let d = self.discover(&c.fixture);
        let (passed, detail) = match (d.registry.get(&c.skill), c.capability.parse::<Capability>())
        {
            (None, _) => (false, format!("skill {:?} not found", c.skill)),
            (_, Err(e)) => (false, e.to_string()),
            (Some(skill), Ok(cap)) => {
                let decision = skill.check(cap);
                let got = if decision.is_allowed() {
                    Verdict::Allow
                } else {
                    Verdict::Deny
                };
                (got == c.expect, decision.reason)
            }
        };
        result(&c.id, "permission", &c.description, passed, detail)
    }

    fn validate(&self, c: &ValidateCase) -> CaseResult {
        let d = self.discover(&c.fixture);
        let report = validate(&d);
        let got = codes(report.diagnostics.iter().map(|x| x.code));
        let want = sorted(c.expect_codes.clone());
        let names: Vec<String> = d.registry.iter().map(|s| s.name().to_owned()).collect();
        let skills_ok = c.expect_skills.as_ref().is_none_or(|w| *w == names);
        let passed = report.is_ok() == c.expect_ok && got == want && skills_ok;
        let mut detail = format!(
            "ok={} skills={} errors={} warnings={} codes={got:?}",
            report.is_ok(),
            names.len(),
            report.errors(),
            report.warnings()
        );
        if !passed {
            detail.push_str(&format!(
                " (expected ok={} codes={want:?} skills={:?})",
                c.expect_ok, c.expect_skills
            ));
            for diag in report.diagnostics.iter().take(10) {
                detail.push_str(&format!(
                    " | {} {}: {}",
                    diag.code, diag.location, diag.message
                ));
            }
        }
        result(&c.id, "validate", &c.description, passed, detail)
    }

    fn skill_eval(&self, c: &SkillEvalCase) -> Vec<CaseResult> {
        let d = self.discover(&c.fixture);
        let mut out = Vec::new();
        for skill in d.registry.iter() {
            let m = skill.manifest();
            if m.eval_cases.is_empty() {
                out.push(result(
                    &format!("{}/{}", c.id, skill.name()),
                    "skill_eval",
                    &c.description,
                    false,
                    format!("skill {} declares no eval_cases", skill.name()),
                ));
            }
            for (i, case) in m.eval_cases.iter().enumerate() {
                let is_router = m.kind == SkillKind::Router;
                let target = case
                    .skill
                    .clone()
                    .unwrap_or_else(|| skill.name().to_owned());
                let invoker = if !is_router && m.invocation.model_invocable {
                    Invoker::Model
                } else {
                    Invoker::User
                };
                let request = RouteRequest {
                    task: case.task.clone(),
                    intents: case.intents.clone(),
                    invoker,
                    router: is_router.then(|| skill.name().to_owned()),
                };
                let id = format!("{}/{}#{}", c.id, skill.name(), i + 1);
                let (passed, detail) = match SkillRouter.route(&d.registry, &request) {
                    Err(e) => (false, format!("routing error: {e}")),
                    Ok(r) => {
                        let ranked = r.ranked.iter().any(|x| x.name == target);
                        let passed = match case.expect {
                            EvalExpectation::Top => r.top() == Some(target.as_str()),
                            EvalExpectation::Ranked => ranked,
                            EvalExpectation::NotRanked => !ranked,
                        };
                        let top = r.ranked.first().map_or_else(
                            || "none".to_owned(),
                            |t| format!("{}:{}", t.name, t.score),
                        );
                        (
                            passed,
                            format!(
                                "{:?} -> expect {} {:?} as {}; top={top}",
                                case.task,
                                target,
                                case.expect,
                                match invoker {
                                    Invoker::User => "user",
                                    Invoker::Model => "model",
                                }
                            ),
                        )
                    }
                };
                out.push(result(&id, "skill_eval", &c.description, passed, detail));
            }
        }
        if out.is_empty() {
            out.push(result(
                &c.id,
                "skill_eval",
                &c.description,
                false,
                "no skills found".to_owned(),
            ));
        }
        out
    }
}

fn route_error_code(e: &RouteError) -> &'static str {
    match e {
        RouteError::UnknownRouter(_) => "unknown_router",
        RouteError::NotARouter(_) => "not_a_router",
        RouteError::RouterNotInvocable(..) => "router_not_invocable",
    }
}
