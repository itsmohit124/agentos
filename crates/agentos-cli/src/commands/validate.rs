//! `agentos validate`: detect providers, run them, show the last result, record a regression.

use std::fmt::Write as _;
use std::process::ExitCode;

use agentos_config::OutputFormat;
use agentos_feedback::{
    ValidationKind, ValidationResult, detect, last, load_project_config, record_regression,
    run_and_store,
};
use agentos_permissions::{Capability, PermissionSet, PermissionSubject};
use agentos_tools::ProcessRunner;
use anyhow::Context;
use serde::Serialize;

use crate::app::App;
use crate::cli::{ValidateArgs, ValidateCommand, ValidateKind};
use crate::output;

/// Exit code when validation did not pass.
const EXIT_FAILED: u8 = 3;

struct Operator {
    permissions: PermissionSet,
}

impl PermissionSubject for Operator {
    fn subject(&self) -> String {
        "operator validate".to_owned()
    }

    fn permissions(&self) -> &PermissionSet {
        &self.permissions
    }
}

/// Runs `agentos validate`.
pub fn run(app: &App, args: &ValidateArgs, format: OutputFormat) -> anyhow::Result<ExitCode> {
    let root = &app.root.path;
    match &args.command {
        Some(ValidateCommand::Detect) => {
            let config = load_project_config(&root.join(".agentos/config.toml"))
                .with_context(|| format!("cannot read validation config in {}", root.display()))?;
            let providers = detect(root, &config)?;
            let text = if providers.is_empty() {
                "no providers\n".to_owned()
            } else {
                let mut out = String::new();
                for provider in &providers {
                    let caps = provider
                        .capabilities
                        .iter()
                        .map(|kind| kind.as_str())
                        .collect::<Vec<_>>()
                        .join(",");
                    let _ = writeln!(out, "{}  {caps}", provider.id);
                }
                out
            };
            emit(format, &providers, &text)?;
            Ok(ExitCode::SUCCESS)
        }
        Some(ValidateCommand::Last) => {
            let result = last(root)?;
            let failed = !result.passed;
            emit(format, &result, &render_result(&result))?;
            Ok(if failed {
                ExitCode::from(EXIT_FAILED)
            } else {
                ExitCode::SUCCESS
            })
        }
        Some(ValidateCommand::RecordRegression { id }) => {
            let path = record_regression(root, id, &root.join("evals/feedback"))?;
            #[derive(Serialize)]
            struct Out {
                id: String,
                path: String,
            }
            let out = Out {
                id: id.clone(),
                path: path.display().to_string(),
            };
            emit(
                format,
                &out,
                &format!("recorded {} -> {}\n", id, path.display()),
            )?;
            Ok(ExitCode::SUCCESS)
        }
        None => {
            let kind = args.kind.map(to_kind).unwrap_or(ValidationKind::UnitTest);
            if kind == ValidationKind::Architecture {
                anyhow::bail!("kind architecture is reported by `agentos architecture analyze`");
            }
            let operator = Operator {
                permissions: if kind == ValidationKind::Browser {
                    [Capability::Browser].into_iter().collect()
                } else {
                    [Capability::Shell].into_iter().collect()
                },
            };
            let results = run_and_store(
                root,
                kind,
                &ProcessRunner,
                &operator,
                args.task.clone(),
                args.context_pack.clone(),
            )?;
            let message = if results.is_empty() {
                Some(format!("no provider supports {kind}"))
            } else {
                None
            };
            let failed = message.is_some() || results.iter().any(|result| !result.passed);
            #[derive(Serialize)]
            struct Out<'a> {
                results: &'a [ValidationResult],
                message: Option<String>,
            }
            let body = Out {
                results: &results,
                message: message.clone(),
            };
            let mut text = render_results(&results);
            if let Some(message) = &message {
                let _ = writeln!(text, "{message}");
            }
            emit(format, &body, &text)?;
            Ok(if failed {
                ExitCode::from(EXIT_FAILED)
            } else {
                ExitCode::SUCCESS
            })
        }
    }
}

fn to_kind(kind: ValidateKind) -> ValidationKind {
    match kind {
        ValidateKind::Typecheck => ValidationKind::Typecheck,
        ValidateKind::Lint => ValidationKind::Lint,
        ValidateKind::UnitTest => ValidationKind::UnitTest,
        ValidateKind::IntegrationTest => ValidationKind::IntegrationTest,
        ValidateKind::Build => ValidationKind::Build,
        ValidateKind::Architecture => ValidationKind::Architecture,
        ValidateKind::Browser => ValidationKind::Browser,
    }
}

fn render_results(results: &[ValidationResult]) -> String {
    if results.is_empty() {
        return String::new();
    }
    results.iter().map(render_result).collect()
}

fn render_result(result: &ValidationResult) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{} {} {} exit {}",
        result.provider,
        result.kind,
        result.outcome.as_str(),
        result
            .exit_status
            .map(|code| code.to_string())
            .unwrap_or_else(|| "-".to_owned())
    );
    for diagnostic in &result.diagnostics {
        match (&diagnostic.file, diagnostic.line) {
            (Some(file), Some(line)) => {
                let _ = writeln!(out, "  {file}:{line} {}", diagnostic.summary);
            }
            _ => {
                let _ = writeln!(out, "  {}", diagnostic.summary);
            }
        }
    }
    out
}

fn emit(format: OutputFormat, value: &impl serde::Serialize, text: &str) -> anyhow::Result<()> {
    match format {
        OutputFormat::Json => output::json(value),
        OutputFormat::Text => output::text(text),
    }
}
