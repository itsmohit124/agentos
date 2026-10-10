//! Test utilities for AgentOS (used only as a dev-dependency).
//!
//! - [`TempProject`]: builds a throw-away project directory, optionally a Git repository.
//! - [`CliRunner`]: runs the `agentos` binary with a hermetic environment via `assert_cmd`.
//! - [`fixtures_dir`], [`fixture_path`], [`copy_fixture`]: load repositories from `fixtures/`.

use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

/// A temporary project directory, deleted on drop.
#[derive(Debug)]
pub struct TempProject {
    dir: TempDir,
    path: PathBuf,
}

impl Default for TempProject {
    fn default() -> Self {
        Self::new()
    }
}

impl TempProject {
    /// Creates an empty temporary directory.
    ///
    /// # Panics
    /// If the directory cannot be created.
    pub fn new() -> Self {
        let dir = tempfile::Builder::new()
            .prefix("agentos-test-")
            .tempdir()
            .expect("create temp dir");
        let path = dir.path().canonicalize().expect("canonicalize temp dir");
        Self { dir, path }
    }

    /// Initialises a Git repository in the project.
    ///
    /// # Panics
    /// If `git init` fails.
    pub fn with_git(self) -> Self {
        let status = Command::new("git")
            .args(["init", "-q"])
            .current_dir(&self.path)
            .status()
            .expect("run git init (tests require git on PATH)");
        assert!(status.success(), "git init failed");
        self
    }

    /// Writes `contents` to `relative`, creating parent directories.
    ///
    /// # Panics
    /// On I/O failure.
    pub fn with_file(self, relative: impl AsRef<Path>, contents: impl AsRef<[u8]>) -> Self {
        self.write(relative, contents);
        self
    }

    /// Creates the directory `relative` (and parents).
    ///
    /// # Panics
    /// On I/O failure.
    pub fn with_dir(self, relative: impl AsRef<Path>) -> Self {
        std::fs::create_dir_all(self.path.join(relative)).expect("create dir");
        self
    }

    /// Writes `contents` to `relative`, creating parent directories.
    ///
    /// # Panics
    /// On I/O failure.
    pub fn write(&self, relative: impl AsRef<Path>, contents: impl AsRef<[u8]>) {
        let path = self.path.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create parent dir");
        }
        std::fs::write(path, contents).expect("write file");
    }

    /// Canonical absolute path of the project.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Absolute path of `relative` inside the project.
    pub fn join(&self, relative: impl AsRef<Path>) -> PathBuf {
        self.path.join(relative)
    }

    /// Reads `relative` as bytes.
    ///
    /// # Panics
    /// If the file cannot be read.
    pub fn read(&self, relative: impl AsRef<Path>) -> Vec<u8> {
        std::fs::read(self.path.join(relative)).expect("read file")
    }

    /// Keeps the directory on disk after drop and returns its path (for debugging).
    pub fn keep(self) -> PathBuf {
        let _ = self.dir.keep();
        self.path
    }
}

/// Runs a CLI binary with a hermetic environment.
#[derive(Debug, Clone)]
pub struct CliRunner {
    bin: PathBuf,
}

impl CliRunner {
    /// Runner for the binary at `bin` (typically `env!("CARGO_BIN_EXE_agentos")`).
    pub fn new(bin: impl Into<PathBuf>) -> Self {
        Self { bin: bin.into() }
    }

    /// A command running in `cwd` with every inherited `AGENTOS_*` and `GIT_*` variable removed.
    pub fn command(&self, cwd: &Path) -> assert_cmd::Command {
        let mut cmd = assert_cmd::Command::new(&self.bin);
        cmd.current_dir(cwd).env("NO_COLOR", "1").env("LC_ALL", "C");
        for (key, _) in std::env::vars_os() {
            let key_str = key.to_string_lossy();
            if key_str.starts_with("AGENTOS_") || key_str.starts_with("GIT_") {
                cmd.env_remove(&key);
            }
        }
        cmd
    }
}

/// Absolute path of the workspace `fixtures/` directory.
pub fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

/// Absolute path of fixture `name`.
///
/// # Panics
/// If the fixture does not exist.
pub fn fixture_path(name: &str) -> PathBuf {
    let path = fixtures_dir().join(name);
    assert!(
        path.is_dir(),
        "fixture {name:?} not found at {}",
        path.display()
    );
    path.canonicalize().expect("canonicalize fixture path")
}

/// Copies fixture `name` into a fresh [`TempProject`] so tests can modify it freely.
///
/// # Panics
/// If the fixture is missing or cannot be copied.
pub fn copy_fixture(name: &str) -> TempProject {
    let project = TempProject::new();
    copy_dir(&fixture_path(name), project.path());
    project
}

fn copy_dir(from: &Path, to: &Path) {
    let mut entries: Vec<_> = std::fs::read_dir(from)
        .expect("read fixture dir")
        .map(|e| e.expect("read fixture entry"))
        .collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let name = entry.file_name();
        if name == ".venv" || name == "node_modules" || name == "__pycache__" {
            continue;
        }
        let target = to.join(&name);
        let file_type = entry.file_type().expect("fixture entry type");
        if file_type.is_dir() {
            std::fs::create_dir_all(&target).expect("create fixture dir");
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).expect("copy fixture file");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temp_project_builder_writes_files() {
        let p = TempProject::new()
            .with_file("a/b.txt", "hi")
            .with_dir("empty");
        assert_eq!(p.read("a/b.txt"), b"hi");
        assert!(p.join("empty").is_dir());
        assert!(p.path().is_absolute());
    }

    #[test]
    fn temp_project_with_git_has_repo() {
        let p = TempProject::new().with_git();
        assert!(p.join(".git").is_dir());
    }

    #[test]
    fn placeholder_fixture_loads_and_copies() {
        let src = fixture_path("placeholder");
        assert!(src.join("README.md").is_file());

        let copy = copy_fixture("placeholder");
        assert_eq!(
            copy.read("README.md"),
            std::fs::read(src.join("README.md")).unwrap()
        );
        assert_eq!(
            copy.read("src/hello.txt"),
            std::fs::read(src.join("src/hello.txt")).unwrap()
        );
        assert!(copy.join("expected/fixture.json").is_file());
    }

    #[test]
    #[should_panic(expected = "fixture \"does-not-exist\" not found")]
    fn missing_fixture_panics_clearly() {
        fixture_path("does-not-exist");
    }
}
