//! Adapter evaluation. Cases are deterministic and do not call a model or the network.
//!
//! Every case runs against a temporary project. Nothing is written to the AgentOS repository
//! or to the fixtures directory.

use std::fs;
use std::path::{Path, PathBuf};

use agentos_adapter_claude::ClaudeAdapter;
use agentos_adapter_codex::CodexAdapter;
use agentos_adapter_cursor::CursorAdapter;
use agentos_adapter_generic::{
    AdapterError, AgentAdapter, FileActionKind, GenericAdapter, ResultRequest, Support,
    invocation_violations,
};
use agentos_skills::Invoker;
use serde::Deserialize;

use crate::{CaseResult, EvalError, EvalReport};

const TASK: &str = "Add retry support to payment webhook processing";

/// Runs every case in `cases_dir`. `fixtures_dir` must exist and is not modified.
pub fn run(cases_dir: &Path, fixtures_dir: &Path) -> Result<EvalReport, EvalError> {
    if !fixtures_dir.is_dir() {
        return Err(EvalError::Failed(format!(
            "fixtures directory {} does not exist",
            fixtures_dir.display()
        )));
    }
    if !cases_dir.is_dir() {
        return Err(EvalError::NoCases(cases_dir.to_path_buf()));
    }
    let mut files: Vec<PathBuf> = fs::read_dir(cases_dir)
        .map_err(|err| EvalError::Failed(err.to_string()))?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    files.sort();
    if files.is_empty() {
        return Err(EvalError::NoCases(cases_dir.to_path_buf()));
    }
    let mut cases = Vec::new();
    for path in files {
        let before = fs::read(&path).map_err(|err| EvalError::Failed(err.to_string()))?;
        let text = String::from_utf8(before.clone())
            .map_err(|_| EvalError::Failed(format!("{} is not UTF-8", path.display())))?;
        let case: Case = serde_json::from_str(&text).map_err(|err| EvalError::InvalidCase {
            path: path.clone(),
            message: err.to_string(),
        })?;
        let stem = path
            .file_stem()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        if case.id != stem {
            return Err(EvalError::InvalidCase {
                path,
                message: format!("id {:?} must equal the file name {stem:?}", case.id),
            });
        }
        if case.description.trim().is_empty() {
            return Err(EvalError::InvalidCase {
                path,
                message: "description must not be empty".to_owned(),
            });
        }
        let result = execute(&case);
        let after = fs::read(&path).map_err(|err| EvalError::Failed(err.to_string()))?;
        if before != after {
            return Err(EvalError::Failed(format!(
                "evaluation wrote {}",
                path.display()
            )));
        }
        cases.push(result);
    }
    Ok(EvalReport {
        area: "adapters".to_owned(),
        cases_dir: cases_dir.to_path_buf(),
        cases,
    })
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    id: String,
    kind: String,
    description: String,
    adapter: String,
}

fn execute(case: &Case) -> CaseResult {
    let (passed, detail) = match run_case(case) {
        Ok(detail) => (true, detail),
        Err(err) => (false, err),
    };
    CaseResult {
        id: case.id.clone(),
        kind: case.kind.clone(),
        description: case.description.clone(),
        passed,
        detail,
        trace: None,
    }
}

fn run_case(case: &Case) -> Result<String, String> {
    let adapters = open_adapters(&case.adapter)?;
    let mut parts = Vec::new();
    for adapter in adapters {
        let detail = match case.kind.as_str() {
            "install" => check_install(adapter.as_ref()),
            "uninstall" => check_uninstall(adapter.as_ref()),
            "invocation" => check_invocation(adapter.as_ref()),
            "permission" => check_permission(adapter.as_ref()),
            "context" => check_context(adapter.as_ref()),
            "workflow" => check_workflow(adapter.as_ref()),
            "feedback" => check_feedback(adapter.as_ref()),
            "doctor" => check_doctor(adapter.as_ref()),
            "isolation" => check_isolation(adapter.as_ref()),
            "capabilities" => check_capabilities(adapter.as_ref()),
            "malformed" => check_malformed(adapter.as_ref()),
            "claim" => check_claim(adapter.as_ref()),
            other => Err(format!("unknown kind {other:?}")),
        }?;
        parts.push(format!("{}={detail}", adapter.id()));
    }
    Ok(parts.join(" "))
}

fn open_adapters(name: &str) -> Result<Vec<Box<dyn AgentAdapter>>, String> {
    let all: Vec<Box<dyn AgentAdapter>> = vec![
        Box::new(ClaudeAdapter),
        Box::new(CodexAdapter),
        Box::new(CursorAdapter),
        Box::new(GenericAdapter),
    ];
    if name == "all" {
        return Ok(all);
    }
    let found = all.into_iter().find(|adapter| adapter.id() == name);
    found
        .map(|adapter| vec![adapter])
        .ok_or_else(|| format!("unknown adapter {name:?}"))
}

fn check_install(adapter: &dyn AgentAdapter) -> Result<String, String> {
    let dir = tempfile::tempdir().map_err(|err| err.to_string())?;
    let first = adapter.install(dir.path()).map_err(|err| err.to_string())?;
    if first.files.is_empty() {
        return Err("install reported no files".to_owned());
    }
    if first
        .files
        .iter()
        .any(|file| file.action == FileActionKind::Failed)
    {
        return Err("install failed a file".to_owned());
    }
    let second = adapter.install(dir.path()).map_err(|err| err.to_string())?;
    if second
        .files
        .iter()
        .any(|file| file.action != FileActionKind::Unchanged)
    {
        return Err(format!("reinstall changed files: {second:?}"));
    }
    Ok("idempotent".to_owned())
}

fn check_uninstall(adapter: &dyn AgentAdapter) -> Result<String, String> {
    let dir = tempfile::tempdir().map_err(|err| err.to_string())?;
    let path = primary_marker(adapter.id());
    let abs = dir.path().join(&path);
    if let Some(parent) = abs.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let original = b"user content\n";
    fs::write(&abs, original).map_err(|err| err.to_string())?;
    adapter.install(dir.path()).map_err(|err| err.to_string())?;
    let edited = fs::read_to_string(&abs)
        .map_err(|err| err.to_string())?
        .replace("user content", "user content edited");
    fs::write(&abs, &edited).map_err(|err| err.to_string())?;
    adapter
        .uninstall(dir.path())
        .map_err(|err| err.to_string())?;
    let restored = fs::read(&abs).map_err(|err| err.to_string())?;
    if restored != b"user content edited\n" {
        return Err(format!("user edit was not preserved in {path}"));
    }
    let clean = tempfile::tempdir().map_err(|err| err.to_string())?;
    let clean_path = clean.path().join(&path);
    if let Some(parent) = clean_path.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    fs::write(&clean_path, original).map_err(|err| err.to_string())?;
    adapter
        .install(clean.path())
        .map_err(|err| err.to_string())?;
    adapter
        .uninstall(clean.path())
        .map_err(|err| err.to_string())?;
    let bytes = fs::read(&clean_path).map_err(|err| err.to_string())?;
    if bytes != original {
        return Err(format!("uninstall did not restore {path} byte for byte"));
    }
    Ok("preserved".to_owned())
}

fn primary_marker(id: &str) -> String {
    match id {
        "cursor" => ".cursor/rules/agentos.mdc".to_owned(),
        "claude" => "CLAUDE.md".to_owned(),
        "codex" => "AGENTS.md".to_owned(),
        _ => ".agentos/adapters/generic/INSTRUCTIONS.md".to_owned(),
    }
}

fn check_invocation(adapter: &dyn AgentAdapter) -> Result<String, String> {
    let dir = tempfile::tempdir().map_err(|err| err.to_string())?;
    let skill = dir.path().join(".agents/skills/manual-only");
    fs::create_dir_all(&skill).map_err(|err| err.to_string())?;
    fs::write(
        skill.join("SKILL.md"),
        "---\nname: manual-only\ndescription: User only.\nversion: \"0.1.0\"\ninvocation: user\n---\n",
    )
    .map_err(|err| err.to_string())?;
    let exposure = adapter
        .expose_skills(dir.path())
        .map_err(|err| err.to_string())?;
    let manual = exposure
        .skills
        .iter()
        .find(|skill| skill.name == "manual-only")
        .ok_or_else(|| "manual-only was not discovered".to_owned())?;
    if manual.model_invocable || !manual.user_invocable {
        return Err("manual-only invocation flags changed".to_owned());
    }
    let grilling = exposure
        .skills
        .iter()
        .find(|skill| skill.name == "grilling")
        .ok_or_else(|| "grilling was not discovered".to_owned())?;
    if grilling.model_invocable {
        return Err("grilling became model-invocable".to_owned());
    }
    if agentos_adapter_generic::authorize(dir.path(), "manual-only", Invoker::Model).is_ok() {
        return Err("model authorize succeeded for a user-only skill".to_owned());
    }
    adapter.install(dir.path()).map_err(|err| err.to_string())?;
    let sealed = fs::read_to_string(skill.join("SKILL.md")).map_err(|err| err.to_string())?;
    if !sealed.contains("disable-model-invocation: true") || !sealed.contains("invocation: user") {
        return Err("user-only skill was not sealed for Cursor".to_owned());
    }
    let policy =
        fs::read_to_string(skill.join("agents/openai.yaml")).map_err(|err| err.to_string())?;
    if !policy.contains("allow_implicit_invocation: false") {
        return Err("user-only skill was not sealed for Codex".to_owned());
    }
    if agentos_adapter_generic::authorize(dir.path(), "manual-only", Invoker::Model).is_ok() {
        return Err("sealed skill became model-invocable".to_owned());
    }
    for file in instruction_files(dir.path(), adapter.id()) {
        let text = fs::read_to_string(&file).map_err(|err| err.to_string())?;
        let violations = invocation_violations(&text, &exposure);
        if !violations.is_empty() {
            return Err(format!(
                "{} lists user-only skills as model-invocable: {}",
                file.display(),
                violations.join(",")
            ));
        }
        if text.contains("allowed-tools:") {
            return Err(format!("{} grants allowed-tools", file.display()));
        }
    }
    if adapter.id() == "claude" {
        let passthrough =
            fs::read_to_string(dir.path().join(".claude/skills/manual-only/SKILL.md"))
                .map_err(|err| err.to_string())?;
        if !passthrough.contains("disable-model-invocation: true") {
            return Err("claude passthrough allows model invocation".to_owned());
        }
        if passthrough.contains("allowed-tools:") {
            return Err("claude passthrough grants allowed-tools".to_owned());
        }
    }
    Ok("user-only".to_owned())
}

fn instruction_files(root: &Path, id: &str) -> Vec<PathBuf> {
    match id {
        "cursor" => vec![root.join(".cursor/rules/agentos.mdc")],
        "claude" => vec![root.join("CLAUDE.md")],
        "codex" => vec![root.join("AGENTS.md")],
        _ => vec![root.join(".agentos/adapters/generic/INSTRUCTIONS.md")],
    }
}

fn check_permission(adapter: &dyn AgentAdapter) -> Result<String, String> {
    let dir = tempfile::tempdir().map_err(|err| err.to_string())?;
    fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"probe\"\nversion = \"0.0.0\"\nedition = \"2021\"\n",
    )
    .map_err(|err| err.to_string())?;
    let result = adapter
        .report_result(
            dir.path(),
            &ResultRequest {
                kind: "unit_test".to_owned(),
                task_id: Some("perm".to_owned()),
                context_pack_id: None,
                claimed_outcome: None,
                permissions: Vec::new(),
            },
        )
        .map_err(|err| err.to_string())?;
    if result.passed || result.outcome != "refused" || !result.executed {
        return Err(format!("expected refused, got {}", result.outcome));
    }
    Ok("refused".to_owned())
}

fn check_context(adapter: &dyn AgentAdapter) -> Result<String, String> {
    let dir = tempfile::tempdir().map_err(|err| err.to_string())?;
    fs::write(dir.path().join("README.md"), "# sample\n").map_err(|err| err.to_string())?;
    let pack = adapter
        .provide_context(dir.path(), TASK)
        .map_err(|err| err.to_string())?;
    if pack.task != TASK {
        return Err("context pack task was rewritten".to_owned());
    }
    Ok(format!("nodes={}", pack.selected_nodes.len()))
}

fn check_workflow(adapter: &dyn AgentAdapter) -> Result<String, String> {
    let dir = tempfile::tempdir().map_err(|err| err.to_string())?;
    let planned = adapter
        .invoke_workflow(dir.path(), TASK, Some("feature"))
        .map_err(|err| err.to_string())?;
    if planned.workflow != "feature" || planned.plan_id.is_empty() || planned.steps.is_empty() {
        return Err(format!("plan was not produced: {planned:?}"));
    }
    if dir.path().join(".agentos/runs").exists() {
        return Err("workflow invocation started a run".to_owned());
    }
    Ok(planned.plan_id)
}

fn check_feedback(adapter: &dyn AgentAdapter) -> Result<String, String> {
    let dir = tempfile::tempdir().map_err(|err| err.to_string())?;
    fs::write(dir.path().join("README.md"), "# sample\n").map_err(|err| err.to_string())?;
    let result = adapter
        .report_result(
            dir.path(),
            &ResultRequest {
                kind: "architecture".to_owned(),
                task_id: None,
                context_pack_id: None,
                claimed_outcome: None,
                permissions: Vec::new(),
            },
        )
        .map_err(|err| err.to_string())?;
    if !result.executed
        || result.source != "architecture"
        || result.method != "architecture-analyze"
    {
        return Err(format!("feedback did not run the analyzer: {result:?}"));
    }
    if !result.detail.starts_with("findings=") {
        return Err(result.detail);
    }
    Ok(result.detail)
}

fn check_doctor(adapter: &dyn AgentAdapter) -> Result<String, String> {
    let dir = tempfile::tempdir().map_err(|err| err.to_string())?;
    let before = adapter.doctor(dir.path()).map_err(|err| err.to_string())?;
    if before.ok {
        return Err("doctor passed before install".to_owned());
    }
    adapter.install(dir.path()).map_err(|err| err.to_string())?;
    let after = adapter.doctor(dir.path()).map_err(|err| err.to_string())?;
    if !after.ok {
        return Err(format!("doctor failed after install: {after:?}"));
    }
    if after
        .checks
        .iter()
        .any(|check| check.name == "external" && check.status == "fail")
    {
        return Err("missing optional agent failed doctor".to_owned());
    }
    Ok("ok".to_owned())
}

fn check_isolation(adapter: &dyn AgentAdapter) -> Result<String, String> {
    let left = tempfile::tempdir().map_err(|err| err.to_string())?;
    let right = tempfile::tempdir().map_err(|err| err.to_string())?;
    fs::write(right.path().join("KEEP.md"), "untouched\n").map_err(|err| err.to_string())?;
    adapter
        .install(left.path())
        .map_err(|err| err.to_string())?;
    if fs::read_to_string(right.path().join("KEEP.md")).map_err(|err| err.to_string())?
        != "untouched\n"
    {
        return Err("the other project changed".to_owned());
    }
    if right.path().join(".cursor").exists()
        || right.path().join("CLAUDE.md").exists()
        || right.path().join("AGENTS.md").exists()
        || right.path().join(".agentos").exists()
    {
        return Err("install leaked into the other project".to_owned());
    }
    Ok("isolated".to_owned())
}

fn check_capabilities(adapter: &dyn AgentAdapter) -> Result<String, String> {
    let caps = adapter.capabilities();
    if caps.id != adapter.id() {
        return Err("capability id drifted".to_owned());
    }
    for (name, support) in [
        ("install", caps.install),
        ("skills", caps.skills),
        ("context", caps.context),
        ("workflow", caps.workflow),
        ("feedback", caps.feedback),
    ] {
        if support != Support::Supported {
            return Err(format!("{name} is not supported"));
        }
    }
    let expected = if adapter.id() == "generic" {
        Support::Unsupported
    } else {
        Support::Optional
    };
    if caps.external_agent != expected {
        return Err(format!(
            "external agent reported as {:?}",
            caps.external_agent
        ));
    }
    Ok("reported".to_owned())
}

fn check_malformed(adapter: &dyn AgentAdapter) -> Result<String, String> {
    if adapter.id() != "claude" {
        return Err("malformed case is for the claude adapter".to_owned());
    }
    let dir = tempfile::tempdir().map_err(|err| err.to_string())?;
    let path = dir.path().join("CLAUDE.md");
    let broken = "<!-- agentos:begin -->\nA\n<!-- agentos:begin -->\nB\n<!-- agentos:end -->\n";
    fs::write(&path, broken).map_err(|err| err.to_string())?;
    let err = adapter.install(dir.path()).map_err(|err| err.to_string());
    match err {
        Err(message) if message.contains("marker") || message.contains("ambiguous") => {}
        Err(message) => return Err(message),
        Ok(_) => return Err("malformed markers were accepted".to_owned()),
    }
    if fs::read_to_string(&path).map_err(|err| err.to_string())? != broken {
        return Err("malformed file was modified".to_owned());
    }
    Ok("unchanged".to_owned())
}

fn check_claim(adapter: &dyn AgentAdapter) -> Result<String, String> {
    let dir = tempfile::tempdir().map_err(|err| err.to_string())?;
    let err = adapter.report_result(
        dir.path(),
        &ResultRequest {
            kind: "unit_test".to_owned(),
            task_id: None,
            context_pack_id: None,
            claimed_outcome: Some("passed".to_owned()),
            permissions: vec!["shell".to_owned()],
        },
    );
    match err {
        Err(AdapterError::UnverifiedClaim(_)) => Ok("rejected".to_owned()),
        Err(err) => Err(err.to_string()),
        Ok(_) => Err("a claimed pass was recorded".to_owned()),
    }
}
