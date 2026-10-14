//! Operations every adapter delegates to existing AgentOS engines.

use std::path::Path;

use agentos_context::{ContextConfig, NullSemantic, SqliteView, build_with_memory};
use agentos_feedback::{
    ArchitectureConfig, Outcome, ValidationKind, analyze_project, run_and_store,
};
use agentos_knowledge::{KnowledgeStore, domain_scan};
use agentos_memory::{FingerprintIndex, MemoryStore};
use agentos_permissions::{Capability, PermissionSet, PermissionSubject};
use agentos_tools::ProcessRunner;
use agentos_workflow::{PlanInput, plan};

use crate::error::AdapterError;
use crate::instructions;
use crate::managed;
use crate::skills;
use crate::types::{
    AdapterCapabilities, DoctorCheck, DoctorReport, FileActionKind, ReportedResult, ResultRequest,
    SkillExposure, Support, WorkflowInvocation,
};

/// Clock used when memory files already exist.
///
/// Adapter context does not read the wall clock. An empty memory directory is skipped, so a
/// project without memory records is unchanged.
const MEMORY_AT: &str = "1970-01-01T00:00:00Z";

/// Builds a Context Pack through the context engine.
pub fn provide_context(
    project: &Path,
    task: &str,
) -> Result<agentos_context::ContextPack, AdapterError> {
    ensure_dir(project)?;
    domain_scan(project).map_err(|err| AdapterError::Context(err.to_string()))?;
    let store =
        KnowledgeStore::open(project).map_err(|err| AdapterError::Context(err.to_string()))?;
    let memory = memory_candidates(project, &store)?;
    let view = SqliteView::new(&store);
    let config = ContextConfig {
        budget: None,
        cache_dir: Some(project.join(".agentos/context-cache")),
    };
    build_with_memory(&view, task, &config, &NullSemantic, &memory)
        .map_err(|err| AdapterError::Context(err.to_string()))
}

/// Plans a workflow. The plan is not started and no runner is called.
pub fn invoke_workflow(
    project: &Path,
    task: &str,
    workflow: Option<&str>,
) -> Result<WorkflowInvocation, AdapterError> {
    ensure_dir(project)?;
    let discovery = {
        let fs = std::sync::Arc::new(agentos_core::fs::RealFs);
        agentos_skills::SkillLoader::for_project(
            project,
            fs,
            None,
            agentos_skills::builtin::location(),
        )
        .discover()
    };
    let planned = plan(&PlanInput {
        task,
        workflow,
        definition: None,
        registry: &discovery.registry,
        concurrency: None,
        max_fix_iterations: None,
    })
    .map_err(|err| AdapterError::Workflow(err.to_string()))?;
    let mut skill_names: Vec<String> = planned
        .skills
        .iter()
        .map(|skill| skill.name.clone())
        .collect();
    skill_names.sort();
    skill_names.dedup();
    Ok(WorkflowInvocation {
        workflow: planned.workflow,
        plan_id: planned.id,
        selection: planned.selection.rule.clone(),
        steps: planned.steps.iter().map(|step| step.id.clone()).collect(),
        skills: skill_names,
    })
}

/// Forwards `request` to the feedback engine.
///
/// A `claimed_outcome` is rejected. `passed` is true only when the engine reported a pass.
pub fn report_result(
    project: &Path,
    request: &ResultRequest,
) -> Result<ReportedResult, AdapterError> {
    ensure_dir(project)?;
    if let Some(claim) = &request.claimed_outcome {
        return Err(AdapterError::UnverifiedClaim(claim.clone()));
    }
    if request.kind == "architecture" {
        return report_architecture(project);
    }
    let kind = parse_kind(&request.kind)?;
    let permissions = parse_permissions(&request.permissions)?;
    let subject = Grant { permissions };
    let results = run_and_store(
        project,
        kind,
        &ProcessRunner,
        &subject,
        request.task_id.clone(),
        request.context_pack_id.clone(),
    )
    .map_err(|err| AdapterError::Feedback(err.to_string()))?;
    if results.is_empty() {
        return Ok(ReportedResult {
            kind: request.kind.clone(),
            executed: false,
            outcome: "unavailable".to_owned(),
            passed: false,
            detail: "no provider selected".to_owned(),
            source: "feedback".to_owned(),
            method: "provider-selection".to_owned(),
        });
    }
    let passed = results.iter().all(|result| result.passed);
    let outcome = if passed {
        "passed".to_owned()
    } else if results
        .iter()
        .any(|result| result.outcome == Outcome::Refused)
    {
        "refused".to_owned()
    } else {
        results
            .iter()
            .find(|result| !result.passed)
            .map(|result| result.outcome.as_str().to_owned())
            .unwrap_or_else(|| "failed".to_owned())
    };
    let detail = results
        .iter()
        .map(|result| format!("{}={}", result.provider, result.outcome.as_str()))
        .collect::<Vec<_>>()
        .join(" ");
    let source = results
        .first()
        .map(|result| result.provider.clone())
        .unwrap_or_else(|| "feedback".to_owned());
    Ok(ReportedResult {
        kind: request.kind.clone(),
        executed: true,
        outcome,
        passed,
        detail,
        source,
        method: "validation-provider".to_owned(),
    })
}

/// Checks managed files, skill policy, and local interfaces. Does not install or scan.
pub fn doctor(
    project: &Path,
    capabilities: &AdapterCapabilities,
    external_command: Option<&str>,
) -> Result<DoctorReport, AdapterError> {
    ensure_dir(project)?;
    let mut checks = Vec::new();
    let installed = managed::is_installed(project, &capabilities.id)?;
    checks.push(DoctorCheck {
        name: "installed".to_owned(),
        status: if installed { "pass" } else { "fail" }.to_owned(),
        detail: if installed {
            "manifest has files for this adapter".to_owned()
        } else {
            "adapter is not installed".to_owned()
        },
    });
    checks.push(marker_check(project, &capabilities.id, installed)?);
    let exposure = skills::expose(project)?;
    checks.push(DoctorCheck {
        name: "skills".to_owned(),
        status: "pass".to_owned(),
        detail: format!("{} skills", exposure.skills.len()),
    });
    if exposure.diagnostics.is_empty() {
        checks.push(DoctorCheck {
            name: "skill_diagnostics".to_owned(),
            status: "pass".to_owned(),
            detail: "none".to_owned(),
        });
    } else {
        checks.push(DoctorCheck {
            name: "skill_diagnostics".to_owned(),
            status: "warn".to_owned(),
            detail: exposure.diagnostics.join("; "),
        });
    }
    checks.push(invocation_check(project, &capabilities.id, &exposure)?);
    checks.push(DoctorCheck {
        name: "context".to_owned(),
        status: if capabilities.context == Support::Supported {
            "pass"
        } else {
            "fail"
        }
        .to_owned(),
        detail: "context packs come from agentos-context".to_owned(),
    });
    checks.push(DoctorCheck {
        name: "workflow".to_owned(),
        status: "pass".to_owned(),
        detail: format!("workflows {}", agentos_workflow::names().join(" ")),
    });
    checks.push(DoctorCheck {
        name: "feedback".to_owned(),
        status: "pass".to_owned(),
        detail: "architecture analyzer and validation providers".to_owned(),
    });
    let gaps = crate::native::problems(project)?;
    checks.push(DoctorCheck {
        name: "native_invocation".to_owned(),
        status: if gaps.is_empty() { "info" } else { "fail" }.to_owned(),
        detail: if gaps.is_empty() {
            "user-only skills in .agents/skills carry the Cursor and Codex controls. No native agent was executed.".to_owned()
        } else {
            gaps.join("; ")
        },
    });
    let ignored = managed::originals_ignored(project);
    checks.push(DoctorCheck {
        name: "originals_ignore".to_owned(),
        status: if ignored { "pass" } else { "warn" }.to_owned(),
        detail: if ignored {
            ".agentos/.gitignore ignores adapters/originals/".to_owned()
        } else {
            ".agentos/.gitignore does not yet ignore adapters/originals/".to_owned()
        },
    });
    checks.push(external_check(external_command));
    let ok = !checks.iter().any(|check| check.status == "fail");
    Ok(DoctorReport {
        adapter: capabilities.id.clone(),
        ok,
        checks,
    })
}

fn marker_check(
    project: &Path,
    adapter: &str,
    installed: bool,
) -> Result<DoctorCheck, AdapterError> {
    if !installed {
        return Ok(DoctorCheck {
            name: "markers".to_owned(),
            status: "fail".to_owned(),
            detail: "no managed files".to_owned(),
        });
    }
    let paths = managed::installed_paths(project, adapter)?;
    for path in &paths {
        let abs = project.join(path);
        if !abs.is_file() {
            return Ok(DoctorCheck {
                name: "markers".to_owned(),
                status: "fail".to_owned(),
                detail: format!("{path} is missing"),
            });
        }
        let text = match std::fs::read_to_string(&abs) {
            Ok(text) => text,
            Err(err) => {
                return Ok(DoctorCheck {
                    name: "markers".to_owned(),
                    status: "fail".to_owned(),
                    detail: format!("{path}: {err}"),
                });
            }
        };
        if path.ends_with(".json") {
            if serde_json::from_str::<serde_json::Value>(&text).is_err() {
                return Ok(DoctorCheck {
                    name: "markers".to_owned(),
                    status: "fail".to_owned(),
                    detail: format!("{path} is not valid JSON"),
                });
            }
            continue;
        }
        if path.ends_with("SKILL.md") && path.contains(".claude/skills/") {
            continue;
        }
        let begins = text.matches("<!-- agentos:begin -->").count();
        let ends = text.matches("<!-- agentos:end -->").count();
        if begins != 1 || ends != 1 {
            return Ok(DoctorCheck {
                name: "markers".to_owned(),
                status: "fail".to_owned(),
                detail: format!("{path} has malformed markers"),
            });
        }
    }
    Ok(DoctorCheck {
        name: "markers".to_owned(),
        status: "pass".to_owned(),
        detail: format!("{} managed files", paths.len()),
    })
}

fn invocation_check(
    project: &Path,
    adapter: &str,
    exposure: &SkillExposure,
) -> Result<DoctorCheck, AdapterError> {
    let paths = managed::installed_paths(project, adapter).unwrap_or_default();
    let mut violations = Vec::new();
    for path in paths {
        let Ok(text) = std::fs::read_to_string(project.join(&path)) else {
            continue;
        };
        if path.ends_with(".json") {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&text)
                && let Some(rows) = value.get("skills").and_then(|item| item.as_array())
            {
                for row in rows {
                    let name = row.get("name").and_then(|item| item.as_str()).unwrap_or("");
                    let model = row.get("model_invocable").and_then(|item| item.as_bool());
                    if let Some(skill) = exposure.skills.iter().find(|skill| skill.name == name)
                        && model != Some(skill.model_invocable)
                    {
                        violations.push(format!("{path} changed model_invocable for {name}"));
                    }
                }
            }
            continue;
        }
        violations.extend(skills::invocation_violations(&text, exposure));
        if text.contains("## User-invoked only") {
            let listed = skills::user_only_section_names(&text);
            for skill in exposure
                .skills
                .iter()
                .filter(|skill| skill.user_invocable && !skill.model_invocable)
            {
                if !listed.iter().any(|name| name == &skill.name) {
                    violations.push(format!(
                        "{} missing from user-only section of {path}",
                        skill.name
                    ));
                }
            }
        }
        if path.contains(".claude/skills/") && path.ends_with("SKILL.md") {
            if text.contains("allowed-tools:") {
                violations.push(format!("{path} sets allowed-tools"));
            }
            if let Some(name) = skill_name_from_passthrough(&path)
                && let Some(skill) = exposure.skills.iter().find(|skill| skill.name == name)
            {
                let disable = format!("disable-model-invocation: {}", !skill.model_invocable);
                if !text.contains(&disable) {
                    violations.push(format!("{path} invocation flag drifted"));
                }
            }
        }
    }
    violations.sort();
    violations.dedup();
    if violations.is_empty() {
        Ok(DoctorCheck {
            name: "invocation".to_owned(),
            status: "pass".to_owned(),
            detail: "user-only skills are not listed as model-invocable".to_owned(),
        })
    } else {
        Ok(DoctorCheck {
            name: "invocation".to_owned(),
            status: "fail".to_owned(),
            detail: violations.join("; "),
        })
    }
}

fn skill_name_from_passthrough(path: &str) -> Option<&str> {
    let marker = ".claude/skills/";
    let start = path.find(marker)? + marker.len();
    let rest = &path[start..];
    rest.split('/').next()
}

fn external_check(command: Option<&str>) -> DoctorCheck {
    let Some(command) = command else {
        return DoctorCheck {
            name: "external".to_owned(),
            status: "info".to_owned(),
            detail: "no external agent is required".to_owned(),
        };
    };
    if executable_on_path(command) {
        DoctorCheck {
            name: "external".to_owned(),
            status: "pass".to_owned(),
            detail: format!("{command} is on PATH"),
        }
    } else {
        DoctorCheck {
            name: "external".to_owned(),
            status: "info".to_owned(),
            detail: format!("optional dependency unavailable: {command} is not on PATH"),
        }
    }
}

fn executable_on_path(name: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(name);
        if !candidate.is_file() {
            continue;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(meta) = candidate.metadata()
                && meta.permissions().mode() & 0o111 != 0
            {
                return true;
            }
        }
        #[cfg(not(unix))]
        {
            return true;
        }
    }
    false
}

fn report_architecture(project: &Path) -> Result<ReportedResult, AdapterError> {
    let report = analyze_project(project, &ArchitectureConfig::default())
        .map_err(|err| AdapterError::Feedback(err.to_string()))?;
    let count = report.findings.len();
    let passed = count == 0;
    Ok(ReportedResult {
        kind: "architecture".to_owned(),
        executed: true,
        outcome: if passed { "passed" } else { "failed" }.to_owned(),
        passed,
        detail: format!("findings={count}"),
        source: "architecture".to_owned(),
        method: "architecture-analyze".to_owned(),
    })
}

fn memory_candidates(
    project: &Path,
    store: &KnowledgeStore,
) -> Result<Vec<agentos_context::MemoryCandidate>, AdapterError> {
    let dir = project.join(".agentos/memory");
    let has_records = std::fs::read_dir(&dir).ok().is_some_and(|entries| {
        entries.filter_map(|entry| entry.ok()).any(|entry| {
            entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "json")
        })
    });
    if !has_records {
        return Ok(Vec::new());
    }
    let index = FingerprintIndex::from_store(store)
        .map_err(|err| AdapterError::Context(err.to_string()))?;
    let memory =
        MemoryStore::open(project).map_err(|err| AdapterError::Context(err.to_string()))?;
    memory
        .candidates(&index, MEMORY_AT)
        .map_err(|err| AdapterError::Context(err.to_string()))
}

fn parse_kind(kind: &str) -> Result<ValidationKind, AdapterError> {
    kind.parse()
        .map_err(|_| AdapterError::UnsupportedOutcome(kind.to_owned()))
}

fn parse_permissions(values: &[String]) -> Result<PermissionSet, AdapterError> {
    let mut caps = Vec::new();
    for value in values {
        let cap: Capability = value
            .parse()
            .map_err(|_| AdapterError::Feedback(format!("unknown capability {value:?}")))?;
        caps.push(cap);
    }
    Ok(caps.into_iter().collect())
}

struct Grant {
    permissions: PermissionSet,
}

impl PermissionSubject for Grant {
    fn subject(&self) -> String {
        "adapter caller".to_owned()
    }

    fn permissions(&self) -> &PermissionSet {
        &self.permissions
    }
}

fn ensure_dir(project: &Path) -> Result<(), AdapterError> {
    if project.is_dir() {
        Ok(())
    } else {
        Err(AdapterError::NotAProject {
            path: project.to_path_buf(),
        })
    }
}

/// Shared install used by [`crate::AgentAdapter::install`].
pub fn install_adapter(
    project: &Path,
    id: &str,
    writes: &[crate::types::ManagedWrite],
) -> Result<crate::types::InstallReport, AdapterError> {
    tracing::debug!(adapter = id, "install");
    managed::install(project, id, writes)
}

/// Shared uninstall used by [`crate::AgentAdapter::uninstall`].
pub fn uninstall_adapter(
    project: &Path,
    id: &str,
) -> Result<crate::types::UninstallReport, AdapterError> {
    let report = managed::uninstall(project, id)?;
    if report
        .files
        .iter()
        .any(|file| file.action == FileActionKind::Failed)
    {
        tracing::warn!(adapter = id, "uninstall left an unsafe file unchanged");
    }
    Ok(report)
}

/// Re-export of the instruction renderer for adapter crates.
pub fn instructions_for(id: &str, exposure: &SkillExposure) -> String {
    instructions::render(id, exposure)
}
