//! Git detection abstraction.
//!
//! Git is accessed through the [`Git`] trait. [`GitCli`] shells out to the `git` binary;
//! [`FakeGit`] is a deterministic stand-in for tests.

use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Error returned by [`Git`] operations.
#[derive(Debug, thiserror::Error)]
pub enum GitError {
    /// The `git` program is not installed or could not be started.
    #[error("git is not available: {0}")]
    Unavailable(#[source] io::Error),
    /// `git` ran but failed for a reason other than "not a repository".
    #[error("git {args} failed in {}: {stderr}", dir.display())]
    Command {
        /// Arguments passed to git.
        args: String,
        /// Directory git was run in.
        dir: PathBuf,
        /// Trimmed standard error of the git process.
        stderr: String,
    },
}

/// Read-only Git queries needed by AgentOS.
pub trait Git: Send + Sync {
    /// Returns `true` if Git can be used at all.
    fn is_available(&self) -> bool;

    /// Returns the top-level directory of the repository containing `dir`,
    /// or `None` if `dir` is not inside a Git repository.
    fn repo_root(&self, dir: &Path) -> Result<Option<PathBuf>, GitError>;
}

/// [`Git`] implementation that runs the `git` command-line program.
#[derive(Debug, Clone)]
pub struct GitCli {
    program: PathBuf,
}

impl Default for GitCli {
    fn default() -> Self {
        Self::new("git")
    }
}

impl GitCli {
    /// Uses `program` as the git executable.
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
        }
    }

    fn command(&self, dir: &Path) -> Command {
        let mut cmd = Command::new(&self.program);
        cmd.arg("-C")
            .arg(dir)
            .env("LC_ALL", "C")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE");
        cmd
    }
}

impl Git for GitCli {
    fn is_available(&self) -> bool {
        Command::new(&self.program)
            .arg("--version")
            .output()
            .is_ok_and(|o| o.status.success())
    }

    fn repo_root(&self, dir: &Path) -> Result<Option<PathBuf>, GitError> {
        let args = ["rev-parse", "--show-toplevel"];
        let output = self
            .command(dir)
            .args(args)
            .output()
            .map_err(GitError::Unavailable)?;
        if output.status.success() {
            let root = String::from_utf8_lossy(&output.stdout).trim().to_owned();
            return Ok(Some(PathBuf::from(root)));
        }
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        if stderr.contains("not a git repository") {
            return Ok(None);
        }
        Err(GitError::Command {
            args: args.join(" "),
            dir: dir.to_path_buf(),
            stderr,
        })
    }
}

/// Deterministic [`Git`] implementation for tests.
#[derive(Debug, Clone, Default)]
pub struct FakeGit {
    available: bool,
    repos: Vec<PathBuf>,
}

impl FakeGit {
    /// Git is not installed.
    pub fn unavailable() -> Self {
        Self::default()
    }

    /// Git is installed but no directory is inside a repository.
    pub fn without_repos() -> Self {
        Self {
            available: true,
            repos: Vec::new(),
        }
    }

    /// Git is installed and `root` is the top level of a repository.
    pub fn with_repo(root: impl Into<PathBuf>) -> Self {
        Self::without_repos().and_repo(root)
    }

    /// Adds another repository rooted at `root`.
    pub fn and_repo(mut self, root: impl Into<PathBuf>) -> Self {
        self.repos.push(root.into());
        self
    }
}

impl Git for FakeGit {
    fn is_available(&self) -> bool {
        self.available
    }

    fn repo_root(&self, dir: &Path) -> Result<Option<PathBuf>, GitError> {
        if !self.available {
            return Err(GitError::Unavailable(io::ErrorKind::NotFound.into()));
        }
        Ok(self
            .repos
            .iter()
            .filter(|root| dir.starts_with(root))
            .max_by_key(|root| root.components().count())
            .cloned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_git_finds_innermost_repo() {
        let git = FakeGit::with_repo("/a").and_repo("/a/b");
        assert_eq!(
            git.repo_root(Path::new("/a/b/c")).unwrap(),
            Some(PathBuf::from("/a/b"))
        );
        assert_eq!(
            git.repo_root(Path::new("/a/x")).unwrap(),
            Some(PathBuf::from("/a"))
        );
        assert_eq!(git.repo_root(Path::new("/z")).unwrap(), None);
    }

    #[test]
    fn fake_git_unavailable_errors() {
        let git = FakeGit::unavailable();
        assert!(!git.is_available());
        assert!(matches!(
            git.repo_root(Path::new("/a")),
            Err(GitError::Unavailable(_))
        ));
    }

    #[test]
    fn git_cli_missing_program_is_unavailable() {
        let git = GitCli::new("/nonexistent/agentos-test-git");
        assert!(!git.is_available());
        assert!(matches!(
            git.repo_root(Path::new(".")),
            Err(GitError::Unavailable(_))
        ));
    }

    #[test]
    fn git_cli_detects_repo_and_non_repo() {
        let git = GitCli::default();
        assert!(git.is_available(), "tests require git on PATH");
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().canonicalize().unwrap();
        assert_eq!(git.repo_root(&dir).unwrap(), None);

        let status = Command::new("git")
            .args(["init", "-q"])
            .current_dir(&dir)
            .status()
            .unwrap();
        assert!(status.success());
        std::fs::create_dir(dir.join("sub")).unwrap();
        let root = git.repo_root(&dir.join("sub")).unwrap().unwrap();
        assert_eq!(root.canonicalize().unwrap(), dir);
    }
}
