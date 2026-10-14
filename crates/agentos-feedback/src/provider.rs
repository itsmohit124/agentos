//! The validation provider seam.
//!
//! [`ValidationProvider`] is the public interface: detect a project, list capabilities, and run
//! one kind. Command lines and output parsers stay on the concrete providers.

use std::path::Path;

use agentos_permissions::PermissionSubject;
use agentos_tools::{ExecRequest, ToolError, ToolRunner};

use crate::error::FeedbackError;
use crate::kind::ValidationKind;
use crate::result::{CapturedOutput, ValidationResult, refused};

/// What the core asks a technology provider to do.
pub trait ValidationProvider {
    /// Stable id (`cargo`, `pytest`, `jest`, …).
    fn id(&self) -> &'static str;

    /// Kinds this provider can run, in a fixed order.
    fn capabilities(&self) -> &'static [ValidationKind];

    /// True when the project files say this provider applies.
    fn detect(&self, root: &Path) -> bool;

    /// Runs `request.kind`. A missing shell permission becomes a refused result, not a pass.
    fn run(&self, request: &RunRequest<'_>) -> Result<ValidationResult, FeedbackError>;
}

/// Inputs for one run. The provider builds the command; [`ToolRunner`] executes it.
pub struct RunRequest<'a> {
    /// Project root.
    pub root: &'a Path,
    /// Kind to run.
    pub kind: ValidationKind,
    /// Process runtime. Providers must not spawn processes themselves.
    pub tools: &'a dyn ToolRunner,
    /// Permission subject. Shell is required for every process provider.
    pub subject: &'a dyn PermissionSubject,
    /// Optional task id copied onto the result and its feedback items.
    pub task_id: Option<String>,
    /// Optional Context Pack id copied onto the result and its feedback items.
    pub context_pack_id: Option<String>,
    /// Whitespace-separated argv that replaces the provider command. Not a shell script.
    pub command_override: Option<String>,
    /// Kill the process after this long.
    pub timeout: std::time::Duration,
    /// Byte cap for stdout and the same cap for stderr.
    pub max_output_bytes: usize,
}

/// Argv plus extra environment variables.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandPlan {
    /// Program and arguments.
    pub argv: Vec<String>,
    /// Extra environment. Existing variables are kept.
    pub env: Vec<(String, String)>,
}

/// Splits a config override on whitespace. The result is argv, not a shell string.
pub fn split_command(command: &str) -> Result<Vec<String>, FeedbackError> {
    let parts: Vec<String> = command.split_whitespace().map(str::to_owned).collect();
    if parts.is_empty() {
        return Err(FeedbackError::Config(
            "command override is empty".to_owned(),
        ));
    }
    Ok(parts)
}

/// Runs `plan` through `request.tools` and parses the outcome.
pub fn execute(
    provider: &str,
    kind: ValidationKind,
    plan: CommandPlan,
    request: &RunRequest<'_>,
    parse: impl Fn(&CapturedOutput) -> ValidationResult,
) -> Result<ValidationResult, FeedbackError> {
    let Some((program, args)) = plan.argv.split_first() else {
        return Err(FeedbackError::Config(format!(
            "provider {provider} built an empty command"
        )));
    };
    let exec = ExecRequest {
        program: program.clone(),
        args: args.to_vec(),
        cwd: request.root.to_path_buf(),
        env: plan.env,
        timeout: request.timeout,
        max_output_bytes: request.max_output_bytes,
    };
    match request.tools.execute(request.subject, &exec) {
        Err(ToolError::Denied { reason, .. }) => Ok(refused(
            provider,
            kind,
            &request.root.display().to_string(),
            plan.argv,
            &reason,
            request.task_id.clone(),
            request.context_pack_id.clone(),
        )),
        Err(other) => Err(other.into()),
        Ok(outcome) => {
            let crashed = outcome.crashed();
            let truncated = outcome.stdout_truncated || outcome.stderr_truncated;
            let spawn_error = outcome.spawn_error.or(outcome.wait_error);
            let captured = CapturedOutput {
                target: request.root.display().to_string(),
                command: plan.argv,
                exit_code: outcome.exit_code,
                stdout: outcome.stdout,
                stderr: outcome.stderr,
                duration_ms: outcome.duration_ms,
                timed_out: outcome.timed_out,
                crashed,
                spawn_error,
                truncated,
                task_id: request.task_id.clone(),
                context_pack_id: request.context_pack_id.clone(),
            };
            Ok(parse(&captured))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::split_command;

    #[test]
    fn a_command_override_is_argv_and_not_a_shell_script() {
        let argv = split_command("python3 -m pytest; touch pwned").unwrap();
        assert_eq!(
            argv,
            [
                "python3".to_owned(),
                "-m".to_owned(),
                "pytest;".to_owned(),
                "touch".to_owned(),
                "pwned".to_owned()
            ]
        );
        assert!(
            argv.iter()
                .all(|part| part != "sh" && part != "bash" && part != "-c")
        );
    }
}
