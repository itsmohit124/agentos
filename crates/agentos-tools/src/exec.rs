//! Spawn a process only after a shell permission check.

use std::io::Read;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use agentos_permissions::{Capability, PermissionSubject, check};

use crate::error::ToolError;

/// One process request. The caller supplies argv; this runtime does not invoke a shell.
#[derive(Debug, Clone)]
pub struct ExecRequest {
    /// Program name or path. Not expanded by a shell.
    pub program: String,
    /// Arguments, already split.
    pub args: Vec<String>,
    /// Working directory.
    pub cwd: PathBuf,
    /// Extra environment variables. The rest of the process environment is kept.
    pub env: Vec<(String, String)>,
    /// How long the process may run before it is killed.
    pub timeout: Duration,
    /// Maximum bytes kept from stdout, and the same cap for stderr.
    pub max_output_bytes: usize,
}

/// What the process did. Output is already truncated to the cap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecOutcome {
    /// Program that was requested.
    pub program: String,
    /// Arguments that were requested.
    pub args: Vec<String>,
    /// Exit code when the process exited normally. Empty when it was killed or never started.
    pub exit_code: Option<i32>,
    /// Captured stdout, at most `max_output_bytes`.
    pub stdout: String,
    /// Captured stderr, at most `max_output_bytes`.
    pub stderr: String,
    /// True when stdout was longer than the cap.
    pub stdout_truncated: bool,
    /// True when stderr was longer than the cap.
    pub stderr_truncated: bool,
    /// Wall time spent waiting, in milliseconds.
    pub duration_ms: u64,
    /// True when the timeout elapsed and the process was killed.
    pub timed_out: bool,
    /// Why `Command::spawn` failed. Empty when a process was started.
    pub spawn_error: Option<String>,
    /// Why waiting on a started process failed. Empty when the wait completed.
    ///
    /// A signal death has no exit code and no wait error. A `try_wait` I/O error is a wait
    /// error, not a crash.
    pub wait_error: Option<String>,
}

impl ExecOutcome {
    /// True when spawn succeeded and the process was not stopped by the timeout.
    pub fn started(&self) -> bool {
        self.spawn_error.is_none()
    }

    /// True when the process died from a signal. A timeout and a wait error are not crashes.
    pub fn crashed(&self) -> bool {
        self.started() && !self.timed_out && self.wait_error.is_none() && self.exit_code.is_none()
    }
}

/// Runs a command for a subject.
pub trait ToolRunner {
    /// Runs `request` when `subject` declares `shell`.
    fn execute(
        &self,
        subject: &dyn PermissionSubject,
        request: &ExecRequest,
    ) -> Result<ExecOutcome, ToolError>;
}

/// The production runner. It uses `std::process` and does not open a shell.
#[derive(Debug, Default, Clone, Copy)]
pub struct ProcessRunner;

impl ToolRunner for ProcessRunner {
    fn execute(
        &self,
        subject: &dyn PermissionSubject,
        request: &ExecRequest,
    ) -> Result<ExecOutcome, ToolError> {
        let decision = check(subject, Capability::Shell);
        if !decision.is_allowed() {
            tracing::info!(subject = %subject.subject(), "shell permission denied");
            return Err(ToolError::Denied {
                reason: decision.reason,
                capability: Capability::Shell,
            });
        }
        Ok(spawn(request))
    }
}

fn spawn(request: &ExecRequest) -> ExecOutcome {
    let started = Instant::now();
    let mut command = Command::new(&request.program);
    command
        .args(&request.args)
        .current_dir(&request.cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (key, value) in &request.env {
        command.env(key, value);
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(err) => {
            return ExecOutcome {
                program: request.program.clone(),
                args: request.args.clone(),
                exit_code: None,
                stdout: String::new(),
                stderr: String::new(),
                stdout_truncated: false,
                stderr_truncated: false,
                duration_ms: elapsed_ms(started),
                timed_out: false,
                spawn_error: Some(err.to_string()),
                wait_error: None,
            };
        }
    };
    let stdout_pipe = child.stdout.take();
    let stderr_pipe = child.stderr.take();
    let cap = request.max_output_bytes;
    let stdout_handle = thread::spawn(move || read_capped(stdout_pipe, cap));
    let stderr_handle = thread::spawn(move || read_capped(stderr_pipe, cap));
    let (timed_out, status) = match wait_for(&mut child, request.timeout) {
        Ok(result) => result,
        Err(err) => {
            let _ = child.kill();
            let _ = child.wait();
            let _ = stdout_handle.join();
            let _ = stderr_handle.join();
            return ExecOutcome {
                program: request.program.clone(),
                args: request.args.clone(),
                exit_code: None,
                stdout: String::new(),
                stderr: err.to_string(),
                stdout_truncated: false,
                stderr_truncated: false,
                duration_ms: elapsed_ms(started),
                timed_out: false,
                spawn_error: None,
                wait_error: Some(err.to_string()),
            };
        }
    };
    let (stdout, stdout_truncated) = stdout_handle.join().unwrap_or_default();
    let (stderr, stderr_truncated) = stderr_handle.join().unwrap_or_default();
    ExecOutcome {
        program: request.program.clone(),
        args: request.args.clone(),
        exit_code: status.and_then(|status| status.code()),
        stdout,
        stderr,
        stdout_truncated,
        stderr_truncated,
        duration_ms: elapsed_ms(started),
        timed_out,
        spawn_error: None,
        wait_error: None,
    }
}

fn wait_for(
    child: &mut Child,
    timeout: Duration,
) -> std::io::Result<(bool, Option<std::process::ExitStatus>)> {
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok((false, Some(status)));
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let status = child.wait().ok();
            return Ok((true, status));
        }
        thread::sleep(Duration::from_millis(20));
    }
}

fn read_capped(pipe: Option<impl Read>, cap: usize) -> (String, bool) {
    let Some(mut pipe) = pipe else {
        return (String::new(), false);
    };
    let mut kept = Vec::new();
    let mut buf = [0_u8; 8192];
    let mut truncated = false;
    loop {
        match pipe.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                let room = cap.saturating_sub(kept.len());
                if room == 0 {
                    truncated = true;
                    continue;
                }
                let take = n.min(room);
                kept.extend_from_slice(&buf[..take]);
                if take < n {
                    truncated = true;
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
    (String::from_utf8_lossy(&kept).into_owned(), truncated)
}

fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::Duration;

    use agentos_permissions::{Capability, PermissionSet, PermissionSubject};
    use tempfile::tempdir;

    use super::*;

    struct Subject {
        name: &'static str,
        permissions: PermissionSet,
    }

    impl PermissionSubject for Subject {
        fn subject(&self) -> String {
            self.name.to_owned()
        }
        fn permissions(&self) -> &PermissionSet {
            &self.permissions
        }
    }

    fn allow_shell() -> Subject {
        Subject {
            name: "operator validate",
            permissions: [Capability::Shell].into_iter().collect(),
        }
    }

    fn deny_all() -> Subject {
        Subject {
            name: "skill 'silent'",
            permissions: PermissionSet::none(),
        }
    }

    fn request(program: &str, args: &[&str], timeout_ms: u64, cap: usize) -> ExecRequest {
        let dir = tempdir().unwrap();
        ExecRequest {
            program: program.to_owned(),
            args: args.iter().map(|arg| (*arg).to_owned()).collect(),
            cwd: dir.keep(),
            env: Vec::new(),
            timeout: Duration::from_millis(timeout_ms),
            max_output_bytes: cap,
        }
    }

    #[test]
    fn missing_shell_permission_does_not_spawn() {
        let dir = tempdir().unwrap();
        let marker = dir.path().join("marker");
        let request = ExecRequest {
            program: "/usr/bin/touch".into(),
            args: vec![marker.display().to_string()],
            cwd: dir.path().to_path_buf(),
            env: Vec::new(),
            timeout: Duration::from_secs(2),
            max_output_bytes: 1024,
        };
        let err = ProcessRunner.execute(&deny_all(), &request).unwrap_err();
        assert!(matches!(err, ToolError::Denied { .. }));
        assert!(!marker.exists());
    }

    #[test]
    fn nonzero_exit_is_preserved() {
        let outcome = ProcessRunner
            .execute(&allow_shell(), &request("/usr/bin/false", &[], 2_000, 1024))
            .unwrap();
        assert_eq!(outcome.exit_code, Some(1));
        assert!(!outcome.timed_out);
        assert!(outcome.spawn_error.is_none());
        assert!(!outcome.crashed());
    }

    #[test]
    fn timeout_kills_the_process() {
        let outcome = ProcessRunner
            .execute(&allow_shell(), &request("/bin/sleep", &["5"], 200, 1024))
            .unwrap();
        assert!(outcome.timed_out);
        assert!(outcome.duration_ms < 5_000);
        assert!(outcome.spawn_error.is_none());
        assert!(!outcome.crashed());
    }

    #[test]
    fn output_is_capped() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("big.txt");
        fs::write(&file, "x".repeat(50_000)).unwrap();
        let request = ExecRequest {
            program: "/bin/cat".into(),
            args: vec![file.display().to_string()],
            cwd: dir.path().to_path_buf(),
            env: Vec::new(),
            timeout: Duration::from_secs(5),
            max_output_bytes: 100,
        };
        let outcome = ProcessRunner.execute(&allow_shell(), &request).unwrap();
        assert_eq!(outcome.exit_code, Some(0));
        assert!(outcome.stdout_truncated);
        assert!(outcome.stdout.len() <= 100);
        assert!(outcome.stdout.chars().all(|ch| ch == 'x'));
    }

    #[test]
    fn missing_program_is_a_spawn_error() {
        let outcome = ProcessRunner
            .execute(
                &allow_shell(),
                &request("agentos-missing-binary-xyz", &[], 1_000, 1024),
            )
            .unwrap();
        assert!(outcome.spawn_error.is_some());
        assert!(!outcome.started());
        assert_eq!(outcome.exit_code, None);
    }

    #[test]
    fn signal_death_is_a_crash() {
        let outcome = ProcessRunner
            .execute(
                &allow_shell(),
                &request("/bin/sh", &["-c", "kill -ABRT $$"], 2_000, 1024),
            )
            .unwrap();
        assert!(outcome.crashed(), "{outcome:?}");
        assert!(!outcome.timed_out);
        assert!(outcome.wait_error.is_none());
    }

    #[test]
    fn a_wait_error_is_not_signal_death() {
        let outcome = ExecOutcome {
            program: "pytest".into(),
            args: Vec::new(),
            exit_code: None,
            stdout: String::new(),
            stderr: "try_wait: broken pipe".into(),
            stdout_truncated: false,
            stderr_truncated: false,
            duration_ms: 1,
            timed_out: false,
            spawn_error: None,
            wait_error: Some("try_wait: broken pipe".into()),
        };
        assert!(!outcome.crashed());
        assert!(outcome.started());
    }
}
