//! Discovered skills and on-demand loading of their bodies and reference files.

use std::collections::BTreeMap;
use std::fmt;
use std::io::Read;
use std::path::PathBuf;
use std::sync::Arc;

use agentos_core::fs::{FileSystem, FsError};
use agentos_permissions::{Capability, Decision, PermissionSet, PermissionSubject};
use serde::Serialize;

use crate::frontmatter::{self, FrontMatterError};
use crate::loader::Level;
use crate::manifest::SkillManifest;
use crate::resolve::{self, ResolveError};

/// Error loading skill content on demand.
#[derive(Debug, thiserror::Error)]
pub enum SkillError {
    /// No skill with this name is registered.
    #[error("unknown skill {0:?}")]
    UnknownSkill(String),
    /// The file could not be read.
    #[error(transparent)]
    Fs(#[from] FsError),
    /// The file is not valid UTF-8.
    #[error("{0} is not valid UTF-8")]
    NotUtf8(String),
    /// The `SKILL.md` no longer has valid front-matter.
    #[error("{path}: {source}")]
    FrontMatter {
        /// Display path.
        path: String,
        /// Cause.
        source: FrontMatterError,
    },
    /// The reference is not declared in the manifest.
    #[error("skill {skill:?} does not declare reference {reference:?}")]
    UndeclaredReference {
        /// Skill name.
        skill: String,
        /// Requested reference.
        reference: String,
    },
}

/// A lower-priority definition hidden by the registered one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Shadowed {
    /// Its level.
    pub level: Level,
    /// Its display path.
    pub path: String,
}

/// A discovered, valid skill. Holds only the manifest; the body is loaded on demand.
#[derive(Clone)]
pub struct Skill {
    manifest: SkillManifest,
    permissions: PermissionSet,
    level: Level,
    path: PathBuf,
    display_path: String,
    overrides: Vec<Shadowed>,
    fs: Arc<dyn FileSystem>,
}

impl fmt::Debug for Skill {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Skill")
            .field("name", &self.manifest.name)
            .field("level", &self.level)
            .field("path", &self.display_path)
            .finish_non_exhaustive()
    }
}

impl Skill {
    pub(crate) fn new(
        manifest: SkillManifest,
        level: Level,
        path: PathBuf,
        display_path: String,
        fs: Arc<dyn FileSystem>,
    ) -> Self {
        Self {
            permissions: manifest.permission_set(),
            manifest,
            level,
            path,
            display_path,
            overrides: Vec::new(),
            fs,
        }
    }

    pub(crate) fn set_overrides(&mut self, overrides: Vec<Shadowed>) {
        self.overrides = overrides;
    }

    /// Skill name.
    pub fn name(&self) -> &str {
        &self.manifest.name
    }

    /// The validated manifest.
    pub fn manifest(&self) -> &SkillManifest {
        &self.manifest
    }

    /// Discovery level it was loaded from.
    pub fn level(&self) -> Level {
        self.level
    }

    /// Display path of its `SKILL.md`.
    pub fn path(&self) -> &str {
        &self.display_path
    }

    /// Lower-priority definitions this skill overrides, highest first.
    pub fn overrides(&self) -> &[Shadowed] {
        &self.overrides
    }

    /// Checks whether this skill may use `capability` (default deny).
    pub fn check(&self, capability: Capability) -> Decision {
        agentos_permissions::check(self, capability)
    }

    /// Loads the markdown body (everything after the front-matter).
    pub fn load_body(&self) -> Result<String, SkillError> {
        let text = self.read_text(&self.path)?;
        let (_, body) = frontmatter::split(&text).map_err(|source| SkillError::FrontMatter {
            path: self.display_path.clone(),
            source,
        })?;
        Ok(body.to_owned())
    }

    /// Loads a declared reference file.
    pub fn load_reference(&self, reference: &str) -> Result<String, SkillError> {
        if !self.manifest.references.iter().any(|r| r == reference) {
            return Err(SkillError::UndeclaredReference {
                skill: self.manifest.name.clone(),
                reference: reference.to_owned(),
            });
        }
        let dir = self.path.parent().expect("SKILL.md has a parent directory");
        self.read_text(&dir.join(reference))
    }

    fn read_text(&self, path: &std::path::Path) -> Result<String, SkillError> {
        let mut bytes = Vec::new();
        self.fs
            .open_read(path)?
            .read_to_end(&mut bytes)
            .map_err(|e| FsError {
                op: "read",
                path: path.to_path_buf(),
                source: e,
            })?;
        String::from_utf8(bytes).map_err(|_| SkillError::NotUtf8(path.display().to_string()))
    }
}

impl PermissionSubject for Skill {
    fn subject(&self) -> String {
        format!("skill '{}'", self.manifest.name)
    }

    fn permissions(&self) -> &PermissionSet {
        &self.permissions
    }
}

/// The set of usable skills, keyed and iterated by name.
#[derive(Debug, Default)]
pub struct SkillRegistry {
    skills: BTreeMap<String, Skill>,
}

impl SkillRegistry {
    pub(crate) fn new(skills: BTreeMap<String, Skill>) -> Self {
        Self { skills }
    }

    /// The skill called `name`.
    pub fn get(&self, name: &str) -> Option<&Skill> {
        self.skills.get(name)
    }

    /// All skills, sorted by name.
    pub fn iter(&self) -> impl Iterator<Item = &Skill> {
        self.skills.values()
    }

    /// Number of skills.
    pub fn len(&self) -> usize {
        self.skills.len()
    }

    /// Whether there are no skills.
    pub fn is_empty(&self) -> bool {
        self.skills.is_empty()
    }

    /// Execution order for `name`: its transitive `requires` first, `name` last.
    pub fn resolve(&self, name: &str) -> Result<Vec<&Skill>, ResolveError> {
        resolve::resolve_order(self, name)
            .map(|order| order.iter().filter_map(|n| self.get(n)).collect())
    }

    /// Loads the body of skill `name`.
    pub fn load_body(&self, name: &str) -> Result<String, SkillError> {
        self.get(name)
            .ok_or_else(|| SkillError::UnknownSkill(name.to_owned()))?
            .load_body()
    }
}
