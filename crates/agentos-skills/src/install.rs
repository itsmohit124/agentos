//! Installing a skill directory into the project's `.agents/skills/`.

use std::io::Read;
use std::path::{Component, Path, PathBuf};

use agentos_core::fs::{FileSystem, FsError};
use serde::Serialize;

use crate::diagnostic::Diagnostic;
use crate::frontmatter;
use crate::loader::{PROJECT_SKILLS_DIR, SKILL_FILE};
use crate::manifest::{Invocation, SkillPermission, parse_manifest};

/// Maximum directory depth copied from a skill directory.
const MAX_DEPTH: usize = 16;

/// Where to install from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallSource {
    /// A local skill directory.
    Local(PathBuf),
    /// A git URL (not supported yet).
    Remote(String),
}

impl InstallSource {
    /// Classifies `spec`: URLs (`scheme://…`, `git@…`, `….git`) are remote, anything else a path.
    pub fn parse(spec: &str) -> Self {
        if spec.contains("://") || spec.starts_with("git@") || spec.ends_with(".git") {
            Self::Remote(spec.to_owned())
        } else {
            Self::Local(PathBuf::from(spec))
        }
    }
}

/// A completed installation.
#[derive(Debug, Clone, Serialize)]
pub struct Installed {
    /// Skill name.
    pub name: String,
    /// Skill version.
    pub version: String,
    /// Destination directory.
    pub destination: PathBuf,
    /// Copied files, relative to the destination, sorted.
    pub files: Vec<String>,
    /// Capabilities the skill declares (and will be allowed).
    pub permissions: Vec<SkillPermission>,
    /// Who may invoke it.
    pub invocation: Invocation,
}

/// Why installation failed. Nothing is written in any of these cases.
#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    /// Remote install is stubbed.
    #[error(
        "installing from a git URL ({0}) is not supported yet; clone it and run `agentos skills install <local-dir>`"
    )]
    RemoteUnsupported(String),
    /// The source has no `SKILL.md`.
    #[error("{} is not a skill directory (no SKILL.md)", .0.display())]
    NotASkill(PathBuf),
    /// The source manifest is invalid.
    #[error("{} is not a valid skill:\n{}", .path.display(), render(.diagnostics))]
    Invalid {
        /// Source directory.
        path: PathBuf,
        /// Problems.
        diagnostics: Vec<Diagnostic>,
    },
    /// The destination already exists.
    #[error("{} already exists; remove it first to reinstall", .0.display())]
    AlreadyInstalled(PathBuf),
    /// The source tree is deeper than allowed (or contains a symlink loop).
    #[error("{} is nested more than {MAX_DEPTH} levels deep", .0.display())]
    TooDeep(PathBuf),
    /// A path would leave the skill directory.
    #[error("{} escapes the skill directory", .0.display())]
    Escapes(PathBuf),
    /// Filesystem failure.
    #[error(transparent)]
    Fs(#[from] FsError),
}

fn render(diagnostics: &[Diagnostic]) -> String {
    diagnostics
        .iter()
        .map(|d| format!("  [{}] {}: {}", d.code, d.location, d.message))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Validates the skill at `source` and copies it to `<project_root>/.agents/skills/<name>/`.
///
/// The manifest `name` decides the destination directory. Files are read fully before anything
/// is written, and an existing destination is never overwritten. `.git` directories are skipped.
pub fn install(
    fs: &dyn FileSystem,
    source: &InstallSource,
    project_root: &Path,
) -> Result<Installed, InstallError> {
    let dir = match source {
        InstallSource::Remote(url) => return Err(InstallError::RemoteUnsupported(url.clone())),
        InstallSource::Local(dir) => dir,
    };
    let manifest_path = dir.join(SKILL_FILE);
    if !fs.is_dir(dir) || !fs.exists(&manifest_path) {
        return Err(InstallError::NotASkill(dir.clone()));
    }
    let display = manifest_path.display().to_string();
    let text = fs.read_to_string(&manifest_path)?;
    let invalid = |diagnostics| InstallError::Invalid {
        path: dir.clone(),
        diagnostics,
    };
    let (yaml, _) = frontmatter::split(&text).map_err(|e| {
        invalid(vec![Diagnostic::error(
            crate::diagnostic::codes::FRONT_MATTER,
            display.clone(),
            None,
            e.to_string(),
        )])
    })?;
    let manifest = parse_manifest(yaml, None, &display).map_err(invalid)?;
    let missing: Vec<Diagnostic> = manifest
        .references
        .iter()
        .filter(|r| !fs.exists(&dir.join(r)))
        .map(|r| {
            Diagnostic::error(
                crate::diagnostic::codes::MISSING_REFERENCE,
                display.clone(),
                Some(&manifest.name),
                format!("declared reference {r:?} does not exist"),
            )
        })
        .collect();
    if !missing.is_empty() {
        return Err(invalid(missing));
    }

    let skills_root = project_root.join(PROJECT_SKILLS_DIR);
    let destination = skills_root.join(&manifest.name);
    if manifest.name.contains("..")
        || manifest.name.contains('/')
        || manifest.name.contains('\\')
        || !destination.starts_with(&skills_root)
    {
        return Err(InstallError::Escapes(destination));
    }
    if fs.exists(&destination) {
        return Err(InstallError::AlreadyInstalled(destination));
    }

    let mut files = Vec::new();
    collect(fs, dir, dir, 0, &mut files)?;
    for (relative, bytes) in &files {
        let target = destination.join(relative);
        if !target.starts_with(&destination) {
            return Err(InstallError::Escapes(target));
        }
        if let Some(parent) = target.parent() {
            fs.create_dir_all(parent)?;
        }
        fs.write(&target, bytes)?;
    }
    Ok(Installed {
        name: manifest.name.clone(),
        version: manifest.version.to_string(),
        destination,
        files: files.into_iter().map(|(r, _)| r).collect(),
        permissions: manifest.permissions.clone(),
        invocation: manifest.invocation,
    })
}

fn collect(
    fs: &dyn FileSystem,
    base: &Path,
    dir: &Path,
    depth: usize,
    out: &mut Vec<(String, Vec<u8>)>,
) -> Result<(), InstallError> {
    if depth > MAX_DEPTH {
        return Err(InstallError::TooDeep(base.to_path_buf()));
    }
    for path in fs.read_dir(dir)? {
        if path.file_name().is_some_and(|n| n == ".git") {
            continue;
        }
        if is_symlink(&path) {
            return Err(InstallError::Escapes(path));
        }
        if fs.is_dir(&path) {
            collect(fs, base, &path, depth + 1, out)?;
            continue;
        }
        let mut bytes = Vec::new();
        fs.open_read(&path)?
            .read_to_end(&mut bytes)
            .map_err(|e| FsError {
                op: "read",
                path: path.clone(),
                source: e,
            })?;
        let relative = relative_path(base, &path)?;
        out.push((relative, bytes));
    }
    out.sort();
    Ok(())
}

fn relative_path(base: &Path, path: &Path) -> Result<String, InstallError> {
    let relative = path
        .strip_prefix(base)
        .map_err(|_| InstallError::Escapes(path.to_path_buf()))?;
    let mut parts = Vec::new();
    for component in relative.components() {
        match component {
            Component::Normal(part) => parts.push(part.to_string_lossy().into_owned()),
            _ => return Err(InstallError::Escapes(path.to_path_buf())),
        }
    }
    if parts.is_empty() {
        return Err(InstallError::Escapes(path.to_path_buf()));
    }
    Ok(parts.join("/"))
}

fn is_symlink(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentos_core::fs::RealFs;

    #[test]
    fn path_traversal_in_the_skill_name_is_rejected() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("src-skill");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(
            source.join("SKILL.md"),
            "---\nname: \"../evil\"\ndescription: d\nversion: \"0.1.0\"\n---\n# Body\n",
        )
        .unwrap();
        let err = install(&RealFs, &InstallSource::Local(source), root.path()).unwrap_err();
        assert!(
            matches!(err, InstallError::Invalid { .. } | InstallError::Escapes(_)),
            "{err}"
        );
        assert!(!root.path().join("evil").exists());
        assert!(!root.path().join(".agents").join("evil").exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_inside_the_skill_is_not_followed() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let outside = root.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("secret.txt"), b"secret").unwrap();
        let source = root.path().join("src-skill");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(
            source.join("SKILL.md"),
            "---\nname: demo\ndescription: d\nversion: \"0.1.0\"\n---\n# Body\n",
        )
        .unwrap();
        symlink(&outside, source.join("escape")).unwrap();
        let project = root.path().join("project");
        std::fs::create_dir_all(&project).unwrap();
        let err = install(&RealFs, &InstallSource::Local(source), &project).unwrap_err();
        assert!(matches!(err, InstallError::Escapes(_)), "{err}");
        assert!(!project.join(".agents/skills/demo").exists());
        assert!(outside.join("secret.txt").is_file());
    }
}
