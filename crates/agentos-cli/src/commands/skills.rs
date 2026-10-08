//! `agentos skills ...`: list, inspect, validate, install and graph skills.

use std::fmt::Write as _;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use agentos_config::OutputFormat;
use agentos_core::fs::{FileSystem, RealFs};
use agentos_permissions::Capability;
use agentos_skills::{
    Diagnostic, Discovery, InstallSource, Level, Skill, SkillGraph, SkillLoader, SkillLocation,
    builtin, install, validate,
};
use anyhow::{Context, bail};
use serde::Serialize;

use crate::app::App;
use crate::cli::{GraphFormat, SkillsArgs, SkillsCommand};
use crate::output;

/// Exit code when validation finds at least one error.
const EXIT_INVALID: u8 = 3;

/// Runs `agentos skills <command>`.
pub fn run(app: &App, args: &SkillsArgs, format: OutputFormat) -> anyhow::Result<ExitCode> {
    if let SkillsCommand::Install { source } = &args.command {
        return install_cmd(app, source, format);
    }
    let discovery = discover(app, args)?;
    match &args.command {
        SkillsCommand::List => list(&discovery, format),
        SkillsCommand::Inspect { name, brief } => inspect(&discovery, name, *brief, format),
        SkillsCommand::Validate => validate_cmd(&discovery, format),
        SkillsCommand::Graph { format: graph } => graph_cmd(&discovery, *graph, format),
        SkillsCommand::Install { .. } => unreachable!("install does not discover"),
    }
}

fn user_dir(args: &SkillsArgs) -> Option<PathBuf> {
    args.user_skills_dir.clone().or_else(|| {
        std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".agentos/skills"))
    })
}

fn builtin_location(args: &SkillsArgs) -> anyhow::Result<SkillLocation> {
    match &args.builtin_skills_dir {
        None => Ok(builtin::location()),
        Some(dir) => {
            if !RealFs.is_dir(dir) {
                bail!("built-in skills directory {} does not exist", dir.display());
            }
            Ok(
                SkillLocation::new(Level::Builtin, dir.clone(), Arc::new(RealFs))
                    .with_label(builtin::BUILTIN_LABEL),
            )
        }
    }
}

fn discover(app: &App, args: &SkillsArgs) -> anyhow::Result<Discovery> {
    let user = user_dir(args);
    tracing::debug!(user_dir = ?user, builtin_dir = ?args.builtin_skills_dir, "skill locations");
    let loader = SkillLoader::for_project(
        &app.root.path,
        Arc::new(RealFs),
        user.as_deref(),
        builtin_location(args)?,
    );
    Ok(loader.discover())
}

fn invocation_label(skill: &Skill) -> &'static str {
    skill
        .manifest()
        .invocation
        .mode()
        .map_or("none", |m| m.as_str())
}

fn render_diagnostic(d: &Diagnostic) -> String {
    let skill = d
        .skill
        .as_deref()
        .map(|s| format!(" ({s})"))
        .unwrap_or_default();
    format!(
        "{} [{}] {}{skill}: {}",
        d.severity.as_str(),
        d.code,
        d.location,
        d.message
    )
}

fn warn_about_diagnostics(diagnostics: &[Diagnostic]) {
    if diagnostics.is_empty() {
        return;
    }
    let errors = diagnostics.iter().filter(|d| d.is_error()).count();
    output::warn(&format!(
        "{} skill problem(s) ({errors} error(s)); run `agentos skills validate` for details",
        diagnostics.len()
    ));
}

#[derive(Serialize)]
struct ListEntry<'a> {
    name: &'a str,
    version: String,
    level: Level,
    kind: &'static str,
    user_invocable: bool,
    model_invocable: bool,
    description: &'a str,
    path: &'a str,
    overrides: Vec<Level>,
}

#[derive(Serialize)]
struct ListOutput<'a> {
    skills: Vec<ListEntry<'a>>,
    diagnostics: &'a [Diagnostic],
}

fn list(discovery: &Discovery, format: OutputFormat) -> anyhow::Result<ExitCode> {
    let entries: Vec<ListEntry<'_>> = discovery
        .registry
        .iter()
        .map(|s| {
            let m = s.manifest();
            ListEntry {
                name: s.name(),
                version: m.version.to_string(),
                level: s.level(),
                kind: m.kind.as_str(),
                user_invocable: m.invocation.user_invocable,
                model_invocable: m.invocation.model_invocable,
                description: &m.description,
                path: s.path(),
                overrides: s.overrides().iter().map(|o| o.level).collect(),
            }
        })
        .collect();
    match format {
        OutputFormat::Json => output::json(&ListOutput {
            skills: entries,
            diagnostics: &discovery.diagnostics,
        })?,
        OutputFormat::Text => {
            let mut out = String::new();
            let _ = writeln!(out, "Skills: {}", discovery.registry.len());
            let width = discovery
                .registry
                .iter()
                .map(|s| s.name().len())
                .max()
                .unwrap_or(4)
                .max(4);
            let _ = writeln!(
                out,
                "  {:<width$}  {:<7}  {:<7}  {:<6}  {:<6}  DESCRIPTION",
                "NAME", "VERSION", "LEVEL", "KIND", "INVOKE"
            );
            for (s, e) in discovery.registry.iter().zip(&entries) {
                let _ = writeln!(
                    out,
                    "  {:<width$}  {:<7}  {:<7}  {:<6}  {:<6}  {}",
                    e.name,
                    e.version,
                    e.level.as_str(),
                    e.kind,
                    invocation_label(s),
                    e.description
                );
                if !e.overrides.is_empty() {
                    let levels: Vec<&str> = e.overrides.iter().map(|l| l.as_str()).collect();
                    let _ = writeln!(out, "  {:<width$}  overrides: {}", "", levels.join(", "));
                }
            }
            output::text(&out)?;
            warn_about_diagnostics(&discovery.diagnostics);
        }
    }
    Ok(ExitCode::SUCCESS)
}

#[derive(Serialize)]
struct InspectOutput<'a> {
    manifest: &'a agentos_skills::SkillManifest,
    level: Level,
    path: &'a str,
    overrides: &'a [agentos_skills::Shadowed],
    resolved_order: Option<Vec<&'a str>>,
    resolve_error: Option<String>,
    allowed: Vec<&'static str>,
    denied: Vec<&'static str>,
    body: Option<String>,
}

fn inspect(
    discovery: &Discovery,
    name: &str,
    brief: bool,
    format: OutputFormat,
) -> anyhow::Result<ExitCode> {
    let Some(skill) = discovery.registry.get(name) else {
        let related: Vec<String> = discovery
            .diagnostics
            .iter()
            .filter(|d| d.skill.as_deref() == Some(name))
            .map(render_diagnostic)
            .collect();
        if related.is_empty() {
            bail!("no skill named {name:?} (see `agentos skills list`)");
        }
        bail!("skill {name:?} is unavailable:\n  {}", related.join("\n  "));
    };
    let m = skill.manifest();
    let (resolved_order, resolve_error) = match discovery.registry.resolve(name) {
        Ok(order) => (
            Some(order.iter().map(|s| s.name()).collect::<Vec<_>>()),
            None,
        ),
        Err(e) => (None, Some(e.to_string())),
    };
    let (allowed, denied): (Vec<Capability>, Vec<Capability>) = Capability::ALL
        .iter()
        .copied()
        .partition(|c| skill.check(*c).is_allowed());
    let allowed: Vec<&'static str> = allowed.into_iter().map(Capability::as_str).collect();
    let denied: Vec<&'static str> = denied.into_iter().map(Capability::as_str).collect();
    let body = if brief {
        None
    } else {
        Some(
            skill
                .load_body()
                .with_context(|| format!("cannot load the body of skill {name:?}"))?,
        )
    };
    let report = InspectOutput {
        manifest: m,
        level: skill.level(),
        path: skill.path(),
        overrides: skill.overrides(),
        resolved_order,
        resolve_error,
        allowed,
        denied,
        body,
    };
    match format {
        OutputFormat::Json => output::json(&report)?,
        OutputFormat::Text => output::text(&render_inspect(skill, &report))?,
    }
    Ok(ExitCode::SUCCESS)
}

fn yes_no(b: bool) -> &'static str {
    if b { "yes" } else { "no" }
}

fn or_none(items: &[String]) -> String {
    if items.is_empty() {
        "(none)".to_owned()
    } else {
        items.join(", ")
    }
}

fn render_inspect(skill: &Skill, r: &InspectOutput<'_>) -> String {
    let m = r.manifest;
    let mut out = String::new();
    let _ = writeln!(out, "{} {}", m.name, m.version);
    let _ = writeln!(out, "  description:     {}", m.description);
    let _ = writeln!(out, "  kind:            {}", m.kind.as_str());
    let _ = writeln!(out, "  level:           {}", r.level.as_str());
    let _ = writeln!(out, "  path:            {}", r.path);
    let _ = writeln!(
        out,
        "  user-invocable:  {}",
        yes_no(m.invocation.user_invocable)
    );
    let _ = writeln!(
        out,
        "  model-invocable: {}",
        yes_no(m.invocation.model_invocable)
    );
    let requires: Vec<String> = m
        .requires
        .iter()
        .map(|d| format!("{} {}", d.name, d.constraint()))
        .collect();
    let _ = writeln!(out, "  requires:        {}", or_none(&requires));
    match (&r.resolved_order, &r.resolve_error) {
        (Some(order), _) => {
            let _ = writeln!(out, "  resolved order:  {}", order.join(" -> "));
        }
        (None, Some(err)) => {
            let _ = writeln!(out, "  resolved order:  ERROR: {err}");
        }
        (None, None) => {}
    }
    let _ = writeln!(
        out,
        "  recommends:      {}",
        or_none(&m.optional_recommends)
    );
    if !m.routes.is_empty() {
        let _ = writeln!(out, "  routes:          {}", m.routes.join(", "));
    }
    let io = |items: &[agentos_skills::SkillIo]| -> Vec<String> {
        items
            .iter()
            .map(|i| {
                if i.required {
                    i.name.clone()
                } else {
                    format!("{} (optional)", i.name)
                }
            })
            .collect()
    };
    let _ = writeln!(out, "  inputs:          {}", or_none(&io(&m.inputs)));
    let _ = writeln!(out, "  outputs:         {}", or_none(&io(&m.outputs)));
    let _ = writeln!(out, "  tools:           {}", or_none(&m.tools));
    let _ = writeln!(
        out,
        "  permissions:     allow [{}]; deny [{}] (default deny)",
        r.allowed.join(", "),
        r.denied.join(", ")
    );
    let _ = writeln!(out, "  references:      {}", or_none(&m.references));
    let t = &m.triggers;
    let _ = writeln!(out, "  trigger phrases: {}", or_none(&t.phrases));
    let _ = writeln!(out, "  keywords:        {}", or_none(&t.keywords));
    let _ = writeln!(out, "  intents:         {}", or_none(&t.intents));
    if !t.exclude.is_empty() {
        let _ = writeln!(out, "  excludes:        {}", t.exclude.join(", "));
    }
    let _ = writeln!(out, "  eval cases:      {}", m.eval_cases.len());
    if !skill.overrides().is_empty() {
        let shadowed: Vec<String> = skill
            .overrides()
            .iter()
            .map(|o| format!("{} ({})", o.level.as_str(), o.path))
            .collect();
        let _ = writeln!(out, "  overrides:       {}", shadowed.join(", "));
    }
    if let Some(body) = &r.body {
        let _ = writeln!(out);
        out.push_str(body);
        if !body.ends_with('\n') {
            out.push('\n');
        }
    }
    out
}

fn validate_cmd(discovery: &Discovery, format: OutputFormat) -> anyhow::Result<ExitCode> {
    let report = validate(discovery);
    match format {
        OutputFormat::Json => {
            #[derive(Serialize)]
            struct Out<'a> {
                ok: bool,
                errors: usize,
                warnings: usize,
                #[serde(flatten)]
                report: &'a agentos_skills::ValidationReport,
            }
            output::json(&Out {
                ok: report.is_ok(),
                errors: report.errors(),
                warnings: report.warnings(),
                report: &report,
            })?;
        }
        OutputFormat::Text => {
            let mut out = String::new();
            for d in &report.diagnostics {
                let _ = writeln!(out, "  {}", render_diagnostic(d));
            }
            let _ = writeln!(
                out,
                "Validated {} skill(s): {} error(s), {} warning(s)",
                report.skills_checked,
                report.errors(),
                report.warnings()
            );
            let _ = writeln!(
                out,
                "Result: {}",
                if report.is_ok() { "OK" } else { "FAILED" }
            );
            output::text(&out)?;
        }
    }
    Ok(if report.is_ok() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(EXIT_INVALID)
    })
}

fn graph_cmd(
    discovery: &Discovery,
    graph_format: GraphFormat,
    format: OutputFormat,
) -> anyhow::Result<ExitCode> {
    let graph = SkillGraph::from_registry(&discovery.registry);
    match (graph_format, format) {
        (GraphFormat::Dot, OutputFormat::Json) => {
            bail!("--format dot cannot be combined with --json")
        }
        (GraphFormat::Text, OutputFormat::Json) => output::json(&graph)?,
        (GraphFormat::Text, OutputFormat::Text) => output::text(&graph.to_text())?,
        (GraphFormat::Dot, OutputFormat::Text) => output::text(&graph.to_dot())?,
    }
    warn_about_diagnostics(&discovery.diagnostics);
    Ok(ExitCode::SUCCESS)
}

fn install_cmd(app: &App, source: &str, format: OutputFormat) -> anyhow::Result<ExitCode> {
    let parsed = InstallSource::parse(source);
    let installed = install(&RealFs, &parsed, &app.root.path)?;
    match format {
        OutputFormat::Json => output::json(&installed)?,
        OutputFormat::Text => {
            let mut out = String::new();
            let dest = installed
                .destination
                .strip_prefix(&app.root.path)
                .unwrap_or(&installed.destination);
            let _ = writeln!(
                out,
                "Installed {} {} to {}",
                installed.name,
                installed.version,
                dest.display()
            );
            let _ = writeln!(out, "  files:           {}", installed.files.join(", "));
            let perms: Vec<&str> = installed
                .permissions
                .iter()
                .map(|p| p.capability.as_str())
                .collect();
            let _ = writeln!(
                out,
                "  permissions:     {} (everything else is denied)",
                if perms.is_empty() {
                    "(none)".to_owned()
                } else {
                    perms.join(", ")
                }
            );
            let _ = writeln!(
                out,
                "  user-invocable:  {}",
                yes_no(installed.invocation.user_invocable)
            );
            let _ = writeln!(
                out,
                "  model-invocable: {}",
                yes_no(installed.invocation.model_invocable)
            );
            output::text(&out)?;
        }
    }
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use agentos_core::fs::MemFs;
    use agentos_skills::Severity;

    use super::*;

    fn world(files: &[(&str, &str)]) -> Discovery {
        let fs = Arc::new(MemFs::new());
        for (path, text) in files {
            let path = Path::new(path);
            fs.create_dir_all(path.parent().unwrap()).unwrap();
            fs.write(path, text.as_bytes()).unwrap();
        }
        SkillLoader::new()
            .with_location(SkillLocation::new(Level::Project, "/p", fs))
            .discover()
    }

    #[test]
    fn diagnostics_render_with_severity_code_and_skill() {
        let d = world(&[("/p/x/SKILL.md", "no front matter")]);
        let rendered = render_diagnostic(&d.diagnostics[0]);
        assert!(rendered.starts_with("error [front_matter] "), "{rendered}");
        assert_eq!(d.diagnostics[0].severity, Severity::Error);
    }
}
