//! Behaviour tests for the skill system against in-memory filesystems.

use std::io::{self, BufRead, Read};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use agentos_core::fs::{FileSystem, FsError, MemFs};
use agentos_permissions::{Capability, Verdict};

use crate::*;

const BODY: &str = "## When to use\nx\n## Inputs\nx\n## Procedure\n1. step\n## Outputs\nx\n## Must not\nx\n## Validation\nx\n";

fn skill_md(name: &str, extra: &str) -> String {
    format!(
        "---\nname: {name}\ndescription: The {name} skill.\nversion: \"0.1.0\"\n{extra}eval_cases:\n  - task: use {name}\n---\n{BODY}"
    )
}

fn put(fs: &MemFs, path: impl AsRef<Path>, contents: &str) {
    let path = path.as_ref();
    fs.create_dir_all(path.parent().unwrap()).unwrap();
    fs.write(path, contents.as_bytes()).unwrap();
}

/// Builds a project at `/p` (with `.agents/skills`), a user dir `/u` and a built-in dir `/b`.
struct World {
    fs: Arc<MemFs>,
}

impl World {
    fn new() -> Self {
        Self {
            fs: Arc::new(MemFs::new()),
        }
    }

    fn dir(level: Level) -> &'static str {
        match level {
            Level::Project => "/p/.agents/skills",
            Level::Agentos => "/p/.agentos/skills",
            Level::User => "/u",
            Level::Builtin => "/b",
        }
    }

    fn skill(self, level: Level, name: &str, extra: &str) -> Self {
        self.raw(level, name, &skill_md(name, extra))
    }

    fn raw(self, level: Level, dir_name: &str, contents: &str) -> Self {
        put(
            &self.fs,
            format!("{}/{dir_name}/SKILL.md", Self::dir(level)),
            contents,
        );
        self
    }

    fn loader(&self) -> SkillLoader {
        let fs: Arc<dyn FileSystem> = self.fs.clone();
        SkillLoader::for_project(
            Path::new("/p"),
            fs.clone(),
            Some(Path::new("/u")),
            SkillLocation::new(Level::Builtin, "/b", fs).with_label("builtin:"),
        )
    }

    fn discover(&self) -> Discovery {
        self.loader().discover()
    }
}

fn codes_of(diags: &[Diagnostic]) -> Vec<&'static str> {
    let mut c: Vec<_> = diags.iter().map(|d| d.code).collect();
    c.sort_unstable();
    c.dedup();
    c
}

fn names(registry: &SkillRegistry) -> Vec<&str> {
    registry.iter().map(Skill::name).collect()
}

// ---------------------------------------------------------------- discovery

#[test]
fn discovery_priority_project_over_agentos_over_user_over_builtin() {
    let w = World::new()
        .skill(Level::Builtin, "a", "")
        .skill(Level::User, "a", "")
        .skill(Level::Agentos, "a", "")
        .skill(Level::Project, "a", "")
        .skill(Level::Builtin, "b", "")
        .skill(Level::User, "b", "")
        .skill(Level::Builtin, "c", "")
        .skill(Level::Agentos, "d", "");
    let d = w.discover();
    assert!(d.diagnostics.is_empty(), "{:?}", d.diagnostics);
    assert_eq!(names(&d.registry), ["a", "b", "c", "d"]);
    let a = d.registry.get("a").unwrap();
    assert_eq!(a.level(), Level::Project);
    assert_eq!(a.path(), ".agents/skills/a/SKILL.md");
    assert_eq!(
        a.overrides()
            .iter()
            .map(|s| (s.level, s.path.as_str()))
            .collect::<Vec<_>>(),
        [
            (Level::Agentos, ".agentos/skills/a/SKILL.md"),
            (Level::User, "/u/a/SKILL.md"),
            (Level::Builtin, "builtin:a/SKILL.md"),
        ]
    );
    assert_eq!(d.registry.get("b").unwrap().level(), Level::User);
    assert_eq!(d.registry.get("c").unwrap().level(), Level::Builtin);
    assert_eq!(d.registry.get("d").unwrap().level(), Level::Agentos);
    assert!(d.registry.get("c").unwrap().overrides().is_empty());
}

#[test]
fn location_order_of_registration_does_not_matter() {
    let w = World::new()
        .skill(Level::Builtin, "a", "")
        .skill(Level::Project, "a", "");
    let fs: Arc<dyn FileSystem> = w.fs.clone();
    let reversed = SkillLoader::new()
        .with_location(SkillLocation::new(Level::Builtin, "/b", fs.clone()))
        .with_location(SkillLocation::new(Level::Project, "/p/.agents/skills", fs))
        .discover();
    assert_eq!(reversed.registry.get("a").unwrap().level(), Level::Project);
}

#[test]
fn duplicates_at_the_same_level_are_errors_and_make_the_name_unavailable() {
    let w = World::new()
        .skill(Level::Builtin, "foo", "")
        .skill(Level::Project, "foo", "")
        .raw(Level::Project, "foo-copy", &skill_md("foo", ""));
    let d = w.discover();
    assert_eq!(
        codes_of(&d.diagnostics),
        [
            codes::DUPLICATE_SKILL,
            codes::NAME_MISMATCH,
            codes::SHADOWED_BY_BROKEN
        ]
    );
    let dup = d
        .diagnostics
        .iter()
        .find(|x| x.code == codes::DUPLICATE_SKILL)
        .unwrap();
    assert!(
        dup.message.contains(".agents/skills/foo-copy/SKILL.md"),
        "{dup:?}"
    );
    assert!(
        dup.message.contains(".agents/skills/foo/SKILL.md"),
        "{dup:?}"
    );
    assert!(
        d.registry.get("foo").is_none(),
        "must not fall back to builtin"
    );
}

#[test]
fn duplicates_across_two_locations_of_one_level_are_errors() {
    let w = World::new()
        .skill(Level::User, "x", "")
        .skill(Level::Builtin, "x", "");
    let fs: Arc<dyn FileSystem> = w.fs.clone();
    let d = SkillLoader::new()
        .with_location(SkillLocation::new(Level::User, "/u", fs.clone()))
        .with_location(SkillLocation::new(Level::User, "/b", fs))
        .discover();
    assert_eq!(codes_of(&d.diagnostics), [codes::DUPLICATE_SKILL]);
    assert!(d.registry.is_empty());
}

#[test]
fn broken_override_shadows_lower_levels_instead_of_falling_back() {
    let w = World::new().skill(Level::Builtin, "tdd", "").raw(
        Level::Project,
        "tdd",
        "---\nname: tdd\n---\nbody\n",
    );
    let d = w.discover();
    assert!(d.registry.get("tdd").is_none());
    assert_eq!(
        codes_of(&d.diagnostics),
        [codes::SCHEMA, codes::SHADOWED_BY_BROKEN]
    );
    let warn = d
        .diagnostics
        .iter()
        .find(|x| x.code == codes::SHADOWED_BY_BROKEN)
        .unwrap();
    assert_eq!(warn.location, "builtin:tdd/SKILL.md");
    assert_eq!(warn.severity, Severity::Warning);
}

#[test]
fn discovery_ignores_hidden_entries_and_files_and_warns_on_dirs_without_skill_md() {
    let w = World::new().skill(Level::Project, "a", "");
    put(&w.fs, "/p/.agents/skills/README.md", "notes");
    put(&w.fs, "/p/.agents/skills/.hidden/SKILL.md", "garbage");
    w.fs.create_dir_all(Path::new("/p/.agents/skills/empty"))
        .unwrap();
    let d = w.discover();
    assert_eq!(names(&d.registry), ["a"]);
    assert_eq!(codes_of(&d.diagnostics), [codes::MISSING_SKILL_MD]);
    assert_eq!(d.diagnostics[0].severity, Severity::Warning);
}

#[test]
fn missing_locations_are_fine_and_a_file_location_is_an_error() {
    assert!(World::new().discover().registry.is_empty());
    let w = World::new();
    put(&w.fs, "/u", "not a dir");
    assert_eq!(
        codes_of(&w.discover().diagnostics),
        [codes::INVALID_LOCATION]
    );
}

#[test]
fn front_matter_problems_are_reported() {
    let w = World::new()
        .raw(Level::Project, "nofm", "# just markdown\n")
        .raw(Level::Project, "open", "---\nname: open\n");
    let d = w.discover();
    assert_eq!(codes_of(&d.diagnostics), [codes::FRONT_MATTER]);
    assert_eq!(d.diagnostics.len(), 2);
}

#[test]
fn missing_reference_is_an_error_and_existing_reference_is_fine() {
    let w = World::new()
        .skill(Level::Project, "a", "references: [references/guide.md]\n")
        .skill(Level::Project, "b", "references: [references/guide.md]\n");
    put(&w.fs, "/p/.agents/skills/b/references/guide.md", "guide");
    let d = w.discover();
    assert_eq!(names(&d.registry), ["b"]);
    assert_eq!(codes_of(&d.diagnostics), [codes::MISSING_REFERENCE]);
}

#[test]
fn missing_eval_cases_is_an_error_only_for_builtins() {
    let bare =
        |name: &str| format!("---\nname: {name}\ndescription: d\nversion: \"0.1.0\"\n---\n{BODY}");
    let w = World::new()
        .raw(Level::Project, "p", &bare("p"))
        .raw(Level::Builtin, "b", &bare("b"));
    let d = w.discover();
    assert_eq!(names(&d.registry), ["p"]);
    let sev: Vec<_> = d
        .diagnostics
        .iter()
        .map(|x| (x.skill.as_deref().unwrap(), x.severity))
        .collect();
    assert_eq!(sev, [("p", Severity::Warning), ("b", Severity::Error)]);
}

#[test]
fn discovery_is_deterministic() {
    let build = || {
        World::new()
            .skill(Level::Project, "zeta", "requires: [alpha]\n")
            .skill(Level::Builtin, "alpha", "")
            .raw(Level::User, "broken", "nope")
            .discover()
    };
    let (a, b) = (build(), build());
    assert_eq!(names(&a.registry), names(&b.registry));
    assert_eq!(a.diagnostics, b.diagnostics);
}

// ---------------------------------------------------------------- resolver

fn registry(skills: &[(&str, &str)]) -> SkillRegistry {
    let mut w = World::new();
    for (name, extra) in skills {
        w = w.skill(Level::Project, name, extra);
    }
    let d = w.discover();
    assert!(d.diagnostics.is_empty(), "{:?}", d.diagnostics);
    d.registry
}

fn order(r: &SkillRegistry, name: &str) -> Vec<String> {
    r.resolve(name)
        .unwrap()
        .iter()
        .map(|s| s.name().to_owned())
        .collect()
}

#[test]
fn resolver_orders_dependencies_first() {
    let r = registry(&[
        ("interface-design", ""),
        (
            "tdd",
            "requires:\n  - name: interface-design\n    version: \"^0.1\"\n",
        ),
    ]);
    assert_eq!(order(&r, "tdd"), ["interface-design", "tdd"]);
    assert_eq!(order(&r, "interface-design"), ["interface-design"]);
}

#[test]
fn resolver_is_deterministic_regardless_of_declaration_order() {
    let r = registry(&[
        ("base", ""),
        ("left", "requires: [base]\n"),
        ("right", "requires: [base]\n"),
        ("top", "requires: [right, left]\n"),
        ("top2", "requires: [left, right]\n"),
    ]);
    assert_eq!(order(&r, "top"), ["base", "left", "right", "top"]);
    assert_eq!(order(&r, "top2"), ["base", "left", "right", "top2"]);
}

#[test]
fn resolver_reports_cycle_path() {
    let r = registry(&[("a", "requires: [b]\n"), ("b", "requires: [a]\n")]);
    assert_eq!(
        r.resolve("a").unwrap_err(),
        ResolveError::Cycle {
            path: vec!["a".into(), "b".into(), "a".into()]
        }
    );
    assert_eq!(
        r.resolve("b").unwrap_err().to_string(),
        "dependency cycle: b -> a -> b"
    );
    let r = registry(&[
        ("a", "requires: [b]\n"),
        ("b", "requires: [c]\n"),
        ("c", "requires: [b]\n"),
    ]);
    assert_eq!(
        r.resolve("a").unwrap_err().to_string(),
        "dependency cycle: b -> c -> b"
    );
}

#[test]
fn resolver_reports_missing_dependency_and_version_mismatch() {
    let r = registry(&[("a", "requires: [ghost]\n")]);
    assert_eq!(
        r.resolve("a").unwrap_err(),
        ResolveError::MissingDependency {
            skill: "a".into(),
            dependency: "ghost".into()
        }
    );
    let r = registry(&[
        ("base", ""),
        ("a", "requires:\n  - name: base\n    version: \">=1.0\"\n"),
    ]);
    let err = r.resolve("a").unwrap_err();
    assert_eq!(err.code(), codes::VERSION_MISMATCH);
    assert_eq!(
        err.to_string(),
        "skill \"a\" requires base >=1.0, but 0.1.0 is installed"
    );
    assert_eq!(
        r.resolve("nope").unwrap_err(),
        ResolveError::UnknownSkill("nope".into())
    );
}

#[test]
fn validate_reports_each_cycle_once_in_canonical_form() {
    let w = World::new()
        .skill(Level::Project, "b", "requires: [a]\n")
        .skill(Level::Project, "a", "requires: [b]\n")
        .skill(Level::Project, "c", "requires: [a, ghost]\n");
    let report = validate(&w.discover());
    let cycles: Vec<_> = report
        .diagnostics
        .iter()
        .filter(|d| d.code == codes::DEPENDENCY_CYCLE)
        .map(|d| d.message.as_str())
        .collect();
    assert_eq!(cycles, ["dependency cycle: a -> b -> a"]);
    assert_eq!(
        codes_of(&report.diagnostics),
        [codes::DEPENDENCY_CYCLE, codes::MISSING_DEPENDENCY]
    );
    assert!(!report.is_ok());
}

// ---------------------------------------------------------------- invocation

#[test]
fn model_invocable_skill_may_not_require_user_only_skill() {
    let w = World::new()
        .skill(Level::Project, "manual", "")
        .skill(
            Level::Project,
            "auto",
            "invocation: both\nrequires: [manual]\n",
        )
        .skill(Level::Project, "manual-chain", "requires: [manual]\n");
    let report = validate(&w.discover());
    let found: Vec<_> = report
        .diagnostics
        .iter()
        .map(|d| (d.skill.as_deref().unwrap(), d.code))
        .collect();
    assert_eq!(found, [("auto", codes::IMPLICIT_MODEL_INVOCATION)]);
}

#[test]
fn model_invocable_router_may_not_route_to_user_only_skill() {
    let router = |inv: &str| {
        format!(
            "---\nname: r\ndescription: d\nversion: \"0.1.0\"\nkind: router\nroutes: [manual]\n{inv}eval_cases:\n  - task: t\n    skill: manual\n---\n{BODY}"
        )
    };
    let report = |inv: &str| {
        validate(
            &World::new()
                .skill(Level::Project, "manual", "")
                .raw(Level::Project, "r", &router(inv))
                .discover(),
        )
    };
    assert!(report("").is_ok(), "{:?}", report("").diagnostics);
    assert_eq!(
        codes_of(&report("model_invocable: true\n").diagnostics),
        [codes::IMPLICIT_MODEL_INVOCATION]
    );
}

#[test]
fn unknown_optional_recommendation_is_a_warning_not_an_error() {
    let report = validate(
        &World::new()
            .skill(Level::Project, "a", "optional_recommends: [ghost]\n")
            .discover(),
    );
    assert!(report.is_ok(), "{:?}", report.diagnostics);
    let found: Vec<_> = report
        .diagnostics
        .iter()
        .map(|d| (d.code, d.severity))
        .collect();
    assert_eq!(found, [(codes::MISSING_RECOMMENDATION, Severity::Warning)]);
}

#[test]
fn malformed_tool_names_are_rejected_by_the_schema() {
    let d = World::new()
        .skill(Level::Project, "ok", "tools: [test-runner, vcs]\n")
        .skill(Level::Project, "bad", "tools: [\"Test Runner\"]\n")
        .discover();
    assert_eq!(names(&d.registry), ["ok"]);
    assert_eq!(codes_of(&d.diagnostics), [codes::SCHEMA]);
    assert_eq!(d.diagnostics[0].skill.as_deref(), Some("bad"));
}

#[test]
fn router_routes_must_exist_and_not_be_routers() {
    let w = World::new().skill(Level::Project, "a", "").raw(
        Level::Project,
        "r",
        &format!(
            "---\nname: r\ndescription: d\nversion: \"0.1.0\"\nkind: router\nroutes: [a, ghost, r2]\neval_cases:\n  - task: t\n    skill: a\n---\n{BODY}"
        ),
    );
    let w = w.raw(
        Level::Project,
        "r2",
        &format!(
            "---\nname: r2\ndescription: d\nversion: \"0.1.0\"\nkind: router\nroutes: [a]\neval_cases:\n  - task: t\n    skill: a\n---\n{BODY}"
        ),
    );
    let report = validate(&w.discover());
    let msgs: Vec<_> = report
        .diagnostics
        .iter()
        .filter(|d| d.code == codes::INVALID_ROUTE)
        .map(|d| d.message.as_str())
        .collect();
    assert_eq!(
        msgs,
        [
            "route target \"ghost\" is not installed",
            "route target \"r2\" is itself a router"
        ]
    );
}

// ---------------------------------------------------------------- router

fn routing_world() -> SkillRegistry {
    registry(&[
        (
            "tdd",
            "invocation: both\ntriggers:\n  phrases: [write tests first, red green refactor]\n  keywords: [test, failing]\n  intents: [testing]\n",
        ),
        (
            "code-review",
            "invocation: both\ntriggers:\n  phrases: [review the code]\n  keywords: [review, diff]\n  exclude: [architecture]\n",
        ),
        (
            "grilling",
            "triggers:\n  phrases: [grill me]\n  keywords: [challenge]\n",
        ),
        (
            "alpha",
            "invocation: both\ntriggers:\n  keywords: [shared]\n",
        ),
        (
            "beta",
            "invocation: both\ntriggers:\n  keywords: [shared]\n",
        ),
    ])
}

fn route(r: &SkillRegistry, task: &str, invoker: Invoker) -> RouteResult {
    SkillRouter
        .route(r, &RouteRequest::new(task, invoker))
        .unwrap()
}

#[test]
fn router_ranks_with_explained_scores() {
    let r = routing_world();
    let result = route(
        &r,
        "Write tests first for the failing parser",
        Invoker::Model,
    );
    assert_eq!(result.top(), Some("tdd"));
    assert_eq!(
        result.ranked[0],
        RankedSkill {
            name: "tdd".into(),
            score: PHRASE_POINTS + 2 * KEYWORD_POINTS,
            reasons: vec![
                "phrase \"write tests first\" (+10)".into(),
                "keyword \"failing\" (+3)".into(),
                "keyword \"test\" (+3)".into(),
            ],
        }
    );
    assert_eq!(result.ranked.len(), 1);
}

#[test]
fn router_uses_the_skill_name_as_a_phrase() {
    let r = routing_world();
    let result = route(&r, "do a code review", Invoker::User);
    assert_eq!(result.top(), Some("code-review"));
    assert_eq!(
        result.ranked[0].reasons,
        ["name \"code review\" (+10)", "keyword \"review\" (+3)"]
    );
}

#[test]
fn router_breaks_ties_by_name() {
    let r = routing_world();
    let result = route(&r, "the shared thing", Invoker::Model);
    let ranked: Vec<_> = result
        .ranked
        .iter()
        .map(|x| (x.name.as_str(), x.score))
        .collect();
    assert_eq!(ranked, [("alpha", 3), ("beta", 3)]);
}

#[test]
fn router_applies_excludes_and_reports_them() {
    let r = routing_world();
    let result = route(&r, "review the architecture", Invoker::User);
    assert_eq!(result.top(), None);
    assert_eq!(
        result.rejected,
        [RejectedSkill {
            name: "code-review".into(),
            reason: "excluded by \"architecture\"".into()
        }]
    );
}

#[test]
fn router_never_offers_user_only_skills_to_the_model() {
    let r = routing_world();
    let user = route(&r, "grill me on this plan", Invoker::User);
    assert_eq!(user.top(), Some("grilling"));
    let model = route(&r, "grill me on this plan", Invoker::Model);
    assert_eq!(model.top(), None);
    assert_eq!(
        model.rejected,
        [RejectedSkill {
            name: "grilling".into(),
            reason: "not model-invocable (user-invoked only)".into()
        }]
    );
}

#[test]
fn router_uses_intent_labels() {
    let r = routing_world();
    let mut req = RouteRequest::new("please handle this", Invoker::Model);
    req.intents = vec!["Testing".into()];
    let result = SkillRouter.route(&r, &req).unwrap();
    assert_eq!(result.top(), Some("tdd"));
    assert_eq!(result.ranked[0].reasons, ["intent \"testing\" (+20)"]);
}

#[test]
fn router_without_match_returns_nothing() {
    let r = routing_world();
    let result = route(&r, "make coffee", Invoker::User);
    assert!(result.ranked.is_empty() && result.rejected.is_empty());
}

#[test]
fn router_skill_restricts_candidates_and_respects_its_invocation() {
    let w = World::new()
        .skill(Level::Project, "a", "invocation: both\ntriggers:\n  keywords: [thing]\n")
        .skill(Level::Project, "b", "invocation: both\ntriggers:\n  keywords: [thing]\n")
        .raw(
            Level::Project,
            "pick",
            &format!(
                "---\nname: pick\ndescription: d\nversion: \"0.1.0\"\nkind: router\nroutes: [b]\neval_cases:\n  - task: t\n    skill: b\n---\n{BODY}"
            ),
        );
    let d = w.discover();
    let r = &d.registry;
    let mut req = RouteRequest::new("the thing", Invoker::User);
    assert_eq!(SkillRouter.route(r, &req).unwrap().ranked.len(), 2);
    req.router = Some("pick".into());
    let result = SkillRouter.route(r, &req).unwrap();
    assert_eq!(
        result
            .ranked
            .iter()
            .map(|x| x.name.as_str())
            .collect::<Vec<_>>(),
        ["b"]
    );
    req.invoker = Invoker::Model;
    assert_eq!(
        SkillRouter.route(r, &req).unwrap_err(),
        RouteError::RouterNotInvocable("pick".into(), "model")
    );
    req.router = Some("a".into());
    assert_eq!(
        SkillRouter.route(r, &req).unwrap_err(),
        RouteError::NotARouter("a".into())
    );
    req.router = Some("ghost".into());
    assert_eq!(
        SkillRouter.route(r, &req).unwrap_err(),
        RouteError::UnknownRouter("ghost".into())
    );
}

// ---------------------------------------------------------------- permissions

#[test]
fn skill_permissions_are_default_deny_and_not_inherited() {
    let r = registry(&[
        ("plain", ""),
        ("runner", "permissions: [shell, fs.read]\n"),
        ("wrapper", "requires: [runner]\n"),
    ]);
    let plain = r.get("plain").unwrap();
    for cap in Capability::ALL {
        assert_eq!(plain.check(cap).verdict, Verdict::Deny);
    }
    assert_eq!(
        plain.check(Capability::Shell).reason,
        "skill 'plain' does not declare shell (default deny)"
    );
    let runner = r.get("runner").unwrap();
    assert!(runner.check(Capability::Shell).is_allowed());
    assert!(!runner.check(Capability::FsWrite).is_allowed());
    let wrapper = r.get("wrapper").unwrap();
    assert!(
        !wrapper.check(Capability::Shell).is_allowed(),
        "no inheritance"
    );
}

// ---------------------------------------------------------------- progressive disclosure

/// `(path, operation, bytes consumed)` per file access.
type AccessLog = Arc<Mutex<Vec<(PathBuf, &'static str, usize)>>>;

/// A filesystem that records which files were opened and how many bytes were consumed.
struct CountingFs {
    inner: MemFs,
    log: AccessLog,
}

struct CountingReader {
    inner: Box<dyn BufRead + Send>,
    path: PathBuf,
    consumed: usize,
    log: AccessLog,
}

impl Read for CountingReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.consumed += n;
        Ok(n)
    }
}

impl BufRead for CountingReader {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        self.inner.fill_buf()
    }
    fn consume(&mut self, amt: usize) {
        self.consumed += amt;
        self.inner.consume(amt);
    }
}

impl Drop for CountingReader {
    fn drop(&mut self) {
        self.log
            .lock()
            .unwrap()
            .push((self.path.clone(), "open_read", self.consumed));
    }
}

impl FileSystem for CountingFs {
    fn exists(&self, path: &Path) -> bool {
        self.inner.exists(path)
    }
    fn is_dir(&self, path: &Path) -> bool {
        self.inner.is_dir(path)
    }
    fn create_dir_all(&self, path: &Path) -> Result<(), FsError> {
        self.inner.create_dir_all(path)
    }
    fn read_to_string(&self, path: &Path) -> Result<String, FsError> {
        let s = self.inner.read_to_string(path)?;
        self.log
            .lock()
            .unwrap()
            .push((path.to_path_buf(), "read_to_string", s.len()));
        Ok(s)
    }
    fn write(&self, path: &Path, contents: &[u8]) -> Result<(), FsError> {
        self.inner.write(path, contents)
    }
    fn write_new(&self, path: &Path, contents: &[u8]) -> Result<bool, FsError> {
        self.inner.write_new(path, contents)
    }
    fn check_writable(&self, dir: &Path) -> Result<(), FsError> {
        self.inner.check_writable(dir)
    }
    fn read_dir(&self, path: &Path) -> Result<Vec<PathBuf>, FsError> {
        self.inner.read_dir(path)
    }
    fn open_read(&self, path: &Path) -> Result<Box<dyn BufRead + Send + '_>, FsError> {
        let inner = self.inner.open_read(path)?;
        let mut bytes = Vec::new();
        let mut inner = inner;
        inner.read_to_end(&mut bytes).unwrap();
        Ok(Box::new(CountingReader {
            inner: Box::new(io::Cursor::new(bytes)),
            path: path.to_path_buf(),
            consumed: 0,
            log: self.log.clone(),
        }))
    }
}

#[test]
fn discovery_reads_front_matter_only_and_bodies_and_references_on_demand() {
    let inner = MemFs::new();
    let front = "---\nname: big\ndescription: Big skill.\nversion: \"0.1.0\"\nreferences: [references/ref.md]\neval_cases:\n  - task: big\n---\n";
    let body = format!("{BODY}{}", "filler line\n".repeat(10_000));
    put(&inner, "/s/big/SKILL.md", &format!("{front}{body}"));
    put(&inner, "/s/big/references/ref.md", "reference text");
    let log = Arc::new(Mutex::new(Vec::new()));
    let fs = Arc::new(CountingFs {
        inner,
        log: log.clone(),
    });
    let d = SkillLoader::new()
        .with_location(SkillLocation::new(Level::Project, "/s", fs))
        .discover();
    assert!(d.diagnostics.is_empty(), "{:?}", d.diagnostics);
    let skill = d.registry.get("big").unwrap();
    assert_eq!(skill.manifest().description, "Big skill.");

    let events = log.lock().unwrap().clone();
    assert_eq!(
        events,
        [(PathBuf::from("/s/big/SKILL.md"), "open_read", front.len())],
        "discovery must consume only the front-matter and never open references"
    );

    assert_eq!(skill.load_body().unwrap(), body);
    let after_body = log.lock().unwrap().clone();
    assert_eq!(after_body.len(), 2);
    assert_eq!(after_body[1].2, front.len() + body.len());
    assert!(
        !after_body
            .iter()
            .any(|(p, _, _)| p.ends_with("references/ref.md"))
    );

    assert_eq!(
        skill.load_reference("references/ref.md").unwrap(),
        "reference text"
    );
    assert!(matches!(
        skill.load_reference("references/other.md"),
        Err(SkillError::UndeclaredReference { .. })
    ));
    let last = log.lock().unwrap().last().cloned().unwrap();
    assert_eq!(last.0, PathBuf::from("/s/big/references/ref.md"));
}

// ---------------------------------------------------------------- graph

#[test]
fn graph_text_and_dot_are_deterministic() {
    let w = World::new()
        .skill(Level::Builtin, "interface-design", "")
        .skill(
            Level::Project,
            "tdd",
            "requires:\n  - name: interface-design\n    version: \"^0.1\"\noptional_recommends: [ghost]\n",
        )
        .raw(
            Level::Builtin,
            "router",
            &format!(
                "---\nname: router\ndescription: d\nversion: \"0.1.0\"\nkind: router\nroutes: [tdd, interface-design]\neval_cases:\n  - task: t\n    skill: tdd\n---\n{BODY}"
            ),
        );
    let d = w.discover();
    let graph = SkillGraph::from_registry(&d.registry);
    assert_eq!(
        graph.to_text(),
        "Skill graph: 3 skills, 4 edges\n\
         interface-design 0.1.0 [builtin]\n\
         router 0.1.0 [builtin, router]\n  routes -> interface-design\n  routes -> tdd\n\
         tdd 0.1.0 [project]\n  requires -> interface-design ^0.1\n  recommends -> ghost (missing)\n"
    );
    assert_eq!(
        graph.to_dot(),
        "digraph skills {\n  rankdir=LR;\n  node [shape=box];\n\
         \x20 \"interface-design\" [label=\"interface-design\\n0.1.0 (builtin)\"];\n\
         \x20 \"router\" [label=\"router\\n0.1.0 (builtin)\", shape=diamond];\n\
         \x20 \"tdd\" [label=\"tdd\\n0.1.0 (project)\"];\n\
         \x20 \"ghost\" [label=\"ghost\\n(missing)\", style=dashed, color=red];\n\
         \x20 \"router\" -> \"interface-design\" [label=\"routes\", style=dotted];\n\
         \x20 \"router\" -> \"tdd\" [label=\"routes\", style=dotted];\n\
         \x20 \"tdd\" -> \"interface-design\" [label=\"requires ^0.1\", style=solid];\n\
         \x20 \"tdd\" -> \"ghost\" [label=\"recommends\", style=dashed];\n\
         }\n"
    );
    assert_eq!(SkillGraph::from_registry(&w.discover().registry), graph);
}

// ---------------------------------------------------------------- install

#[test]
fn install_copies_the_skill_into_project_skills() {
    let fs = MemFs::new();
    put(
        &fs,
        "/src/my-skill/SKILL.md",
        &skill_md(
            "my-skill",
            "permissions: [fs.read]\nreferences: [references/r.md]\n",
        ),
    );
    put(&fs, "/src/my-skill/references/r.md", "ref");
    put(&fs, "/src/my-skill/.git/HEAD", "ref: x");
    fs.create_dir_all(Path::new("/proj")).unwrap();
    let installed = install(
        &fs,
        &InstallSource::Local("/src/my-skill".into()),
        Path::new("/proj"),
    )
    .unwrap();
    assert_eq!(installed.name, "my-skill");
    assert_eq!(
        installed.destination,
        PathBuf::from("/proj/.agents/skills/my-skill")
    );
    assert_eq!(installed.files, ["SKILL.md", "references/r.md"]);
    assert_eq!(installed.permissions[0].capability, Capability::FsRead);
    assert_eq!(
        fs.read_to_string(Path::new("/proj/.agents/skills/my-skill/references/r.md"))
            .unwrap(),
        "ref"
    );
    assert!(!fs.exists(Path::new("/proj/.agents/skills/my-skill/.git")));

    let again = install(
        &fs,
        &InstallSource::Local("/src/my-skill".into()),
        Path::new("/proj"),
    );
    assert!(matches!(again, Err(InstallError::AlreadyInstalled(_))));
}

#[test]
fn install_uses_the_manifest_name_for_the_destination() {
    let fs = MemFs::new();
    put(&fs, "/src/checkout-v2/SKILL.md", &skill_md("checkout", ""));
    let installed = install(
        &fs,
        &InstallSource::Local("/src/checkout-v2".into()),
        Path::new("/proj"),
    )
    .unwrap();
    assert_eq!(
        installed.destination,
        PathBuf::from("/proj/.agents/skills/checkout")
    );
}

#[test]
fn install_rejects_invalid_sources_without_writing() {
    let fs = MemFs::new();
    put(&fs, "/src/bad/SKILL.md", "---\nname: bad\n---\n");
    put(
        &fs,
        "/src/noref/SKILL.md",
        &skill_md("noref", "references: [gone.md]\n"),
    );
    fs.create_dir_all(Path::new("/src/empty")).unwrap();
    let run = |dir: &str| install(&fs, &InstallSource::Local(dir.into()), Path::new("/proj"));
    assert!(matches!(run("/src/bad"), Err(InstallError::Invalid { .. })));
    assert!(matches!(
        run("/src/noref"),
        Err(InstallError::Invalid { .. })
    ));
    assert!(matches!(run("/src/empty"), Err(InstallError::NotASkill(_))));
    assert!(matches!(
        run("/src/nothing"),
        Err(InstallError::NotASkill(_))
    ));
    assert!(!fs.exists(Path::new("/proj/.agents")));
}

#[test]
fn install_from_git_url_is_a_clear_stub() {
    for spec in [
        "https://github.com/org/skills.git",
        "git@github.com:org/skills.git",
        "ssh://host/repo",
        "skills.git",
    ] {
        let source = InstallSource::parse(spec);
        assert_eq!(source, InstallSource::Remote(spec.into()));
        let err = install(&MemFs::new(), &source, Path::new("/p")).unwrap_err();
        assert!(err.to_string().contains("not supported yet"), "{err}");
    }
    assert_eq!(
        InstallSource::parse("./skills/tdd"),
        InstallSource::Local("./skills/tdd".into())
    );
}

// ---------------------------------------------------------------- built-ins

const BUILTINS: [&str; 10] = [
    "architecture-review",
    "code-review",
    "domain-modeling",
    "engineering-router",
    "grilling",
    "implementation",
    "improve-codebase-architecture",
    "interface-design",
    "research",
    "tdd",
];

fn builtin_discovery() -> Discovery {
    SkillLoader::new()
        .with_location(builtin::location())
        .discover()
}

#[test]
fn builtin_skills_all_load_and_validate() {
    let d = builtin_discovery();
    assert!(d.diagnostics.is_empty(), "{:#?}", d.diagnostics);
    assert_eq!(names(&d.registry), BUILTINS);
    let report = validate(&d);
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    for skill in d.registry.iter() {
        assert!(!skill.manifest().eval_cases.is_empty(), "{}", skill.name());
        assert!(skill.path().starts_with("builtin:"));
    }
}

#[test]
fn builtin_tdd_resolves_after_interface_design() {
    let d = builtin_discovery();
    assert_eq!(order(&d.registry, "tdd"), ["interface-design", "tdd"]);
}

#[test]
fn builtin_router_is_a_user_invoked_router_over_the_nine_skills() {
    let d = builtin_discovery();
    let router = d.registry.get("engineering-router").unwrap();
    assert_eq!(router.manifest().kind, SkillKind::Router);
    assert_eq!(
        router.manifest().invocation.mode(),
        Some(InvocationMode::User)
    );
    let mut routes = router.manifest().routes.clone();
    routes.sort();
    let expected: Vec<&str> = BUILTINS
        .iter()
        .copied()
        .filter(|n| *n != "engineering-router")
        .collect();
    assert_eq!(routes, expected);
    assert!(router.manifest().permissions.is_empty());
}

#[test]
fn builtin_skill_without_shell_is_denied_shell() {
    let d = builtin_discovery();
    let research = d.registry.get("research").unwrap();
    assert!(
        !research
            .manifest()
            .permission_set()
            .contains(Capability::Shell)
    );
    let decision = research.check(Capability::Shell);
    assert_eq!(decision.verdict, Verdict::Deny);
    assert!(
        d.registry
            .get("tdd")
            .unwrap()
            .check(Capability::Shell)
            .is_allowed()
    );
}
