//! Project-root detection.
//!
//! Resolution order: explicit override (`--root`) → Git top-level of the working directory →
//! the working directory itself.

use std::fmt;
use std::path::{Path, PathBuf};

use crate::fs::FileSystem;
use crate::git::{Git, GitError};

/// How the project root was determined.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootSource {
    /// Given explicitly by the caller (e.g. `--root`).
    Override,
    /// Top level of the Git repository containing the working directory.
    GitRoot,
    /// The working directory, because it is not inside a Git repository.
    WorkingDir,
}

impl RootSource {
    /// Stable machine-readable name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Override => "override",
            Self::GitRoot => "git",
            Self::WorkingDir => "cwd",
        }
    }
}

impl fmt::Display for RootSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A detected project root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectRoot {
    /// Absolute path of the project root.
    pub path: PathBuf,
    /// How the root was determined.
    pub source: RootSource,
}

/// Error returned by [`detect_root`].
#[derive(Debug, thiserror::Error)]
pub enum RootError {
    /// The explicit root override is not an existing directory.
    #[error("project root {} is not a directory", .0.display())]
    NotADirectory(PathBuf),
}

/// Detects the project root.
///
/// `cwd` and `override_root` should already be absolute. Git failures other than
/// "not a repository" degrade to the working directory and are logged, so a broken Git setup
/// never makes AgentOS unusable.
pub fn detect_root(
    fs: &dyn FileSystem,
    git: &dyn Git,
    cwd: &Path,
    override_root: Option<&Path>,
) -> Result<ProjectRoot, RootError> {
    if let Some(path) = override_root {
        if !fs.is_dir(path) {
            return Err(RootError::NotADirectory(path.to_path_buf()));
        }
        return Ok(ProjectRoot {
            path: path.to_path_buf(),
            source: RootSource::Override,
        });
    }
    match git.repo_root(cwd) {
        Ok(Some(path)) => {
            return Ok(ProjectRoot {
                path,
                source: RootSource::GitRoot,
            });
        }
        Ok(None) => {}
        Err(GitError::Unavailable(e)) => tracing::debug!("git unavailable: {e}"),
        Err(e) => tracing::warn!("git root detection failed, using working directory: {e}"),
    }
    Ok(ProjectRoot {
        path: cwd.to_path_buf(),
        source: RootSource::WorkingDir,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::MemFs;
    use crate::git::FakeGit;

    fn fs_with(dirs: &[&str]) -> MemFs {
        let fs = MemFs::new();
        for d in dirs {
            fs.create_dir_all(Path::new(d)).unwrap();
        }
        fs
    }

    #[test]
    fn override_wins_over_git() {
        let fs = fs_with(&["/repo/sub", "/elsewhere"]);
        let git = FakeGit::with_repo("/repo");
        let root = detect_root(
            &fs,
            &git,
            Path::new("/repo/sub"),
            Some(Path::new("/elsewhere")),
        )
        .unwrap();
        assert_eq!(root.path, PathBuf::from("/elsewhere"));
        assert_eq!(root.source, RootSource::Override);
    }

    #[test]
    fn override_must_be_a_directory() {
        let fs = fs_with(&["/repo"]);
        let err = detect_root(
            &fs,
            &FakeGit::without_repos(),
            Path::new("/repo"),
            Some(Path::new("/missing")),
        )
        .unwrap_err();
        assert!(matches!(err, RootError::NotADirectory(p) if p == Path::new("/missing")));
    }

    #[test]
    fn git_root_used_from_subdirectory() {
        let fs = fs_with(&["/repo/a/b"]);
        let root = detect_root(
            &fs,
            &FakeGit::with_repo("/repo"),
            Path::new("/repo/a/b"),
            None,
        )
        .unwrap();
        assert_eq!(root.path, PathBuf::from("/repo"));
        assert_eq!(root.source, RootSource::GitRoot);
    }

    #[test]
    fn no_repo_falls_back_to_cwd() {
        let fs = fs_with(&["/plain"]);
        let root = detect_root(&fs, &FakeGit::without_repos(), Path::new("/plain"), None).unwrap();
        assert_eq!(root.path, PathBuf::from("/plain"));
        assert_eq!(root.source, RootSource::WorkingDir);
    }

    #[test]
    fn git_unavailable_falls_back_to_cwd() {
        let fs = fs_with(&["/plain"]);
        let root = detect_root(&fs, &FakeGit::unavailable(), Path::new("/plain"), None).unwrap();
        assert_eq!(root.source, RootSource::WorkingDir);
    }
}
