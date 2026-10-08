//! Skill discovery across prioritized locations.
//!
//! Priority (highest first): project `.agents/skills/` → `.agentos/skills/` → user-level
//! directory → built-in skills. A higher level overrides a lower one. Two definitions of the same
//! name at the same level are an error. A broken or duplicated definition shadows lower levels
//! (the name becomes unavailable) instead of silently falling back to a lower-level copy.
//!
//! Discovery reads only front-matter; bodies and reference files are never read here.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use agentos_core::fs::FileSystem;
use serde::{Deserialize, Serialize};

use crate::diagnostic::{Diagnostic, codes};
use crate::frontmatter::read_front_matter;
use crate::manifest::parse_manifest;
use crate::registry::{Shadowed, Skill, SkillRegistry};

/// File name of a skill manifest.
pub const SKILL_FILE: &str = "SKILL.md";

/// Project-local portable skills directory, relative to the project root.
pub const PROJECT_SKILLS_DIR: &str = ".agents/skills";
/// AgentOS-managed project skills directory, relative to the project root.
pub const AGENTOS_SKILLS_DIR: &str = ".agentos/skills";

/// Discovery level, in priority order (highest first).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    /// `<project>/.agents/skills/`.
    Project,
    /// `<project>/.agentos/skills/`.
    Agentos,
    /// The user-level skills directory.
    User,
    /// Skills shipped with AgentOS.
    Builtin,
}

impl Level {
    /// Stable name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::Agentos => "agentos",
            Self::User => "user",
            Self::Builtin => "builtin",
        }
    }
}

impl fmt::Display for Level {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One directory searched for skills, with the filesystem it lives on.
#[derive(Clone)]
pub struct SkillLocation {
    level: Level,
    dir: PathBuf,
    label: String,
    fs: Arc<dyn FileSystem>,
}

impl fmt::Debug for SkillLocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SkillLocation")
            .field("level", &self.level)
            .field("dir", &self.dir)
            .field("label", &self.label)
            .finish_non_exhaustive()
    }
}

impl SkillLocation {
    /// Location `dir` at `level`. Paths are displayed relative to `dir` under its own name.
    pub fn new(level: Level, dir: impl Into<PathBuf>, fs: Arc<dyn FileSystem>) -> Self {
        let dir = dir.into();
        let label = dir.display().to_string();
        Self {
            level,
            dir,
            label,
            fs,
        }
    }

    /// Replaces the display prefix (e.g. `.agents/skills` or `builtin:`).
    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// The level.
    pub fn level(&self) -> Level {
        self.level
    }

    /// The directory.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The filesystem.
    pub fn fs(&self) -> &Arc<dyn FileSystem> {
        &self.fs
    }

    /// Display form of `path` (inside this location).
    pub fn display(&self, path: &Path) -> String {
        let relative = path
            .strip_prefix(&self.dir)
            .map(|p| {
                p.components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join("/")
            })
            .unwrap_or_else(|_| path.display().to_string());
        if self.label.ends_with(':') || self.label.ends_with('/') {
            format!("{}{relative}", self.label)
        } else {
            format!("{}/{relative}", self.label)
        }
    }
}

/// Result of discovery: the usable skills plus every problem found while loading them.
#[derive(Debug)]
pub struct Discovery {
    /// Skills that loaded successfully and won their name.
    pub registry: SkillRegistry,
    /// Problems found while loading, sorted.
    pub diagnostics: Vec<Diagnostic>,
}

/// Discovers skills in a set of [`SkillLocation`]s.
#[derive(Debug, Default)]
pub struct SkillLoader {
    locations: Vec<SkillLocation>,
}

/// A skill definition found at one level (loaded or broken).
struct Candidate {
    name: String,
    display: String,
    skill: Option<Skill>,
}

impl SkillLoader {
    /// A loader with no locations.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a location. Order of calls does not matter; levels decide priority.
    pub fn with_location(mut self, location: SkillLocation) -> Self {
        self.locations.push(location);
        self
    }

    /// The standard locations for a project: `.agents/skills/`, `.agentos/skills/`, `user_dir`
    /// (if any) and `builtin`.
    pub fn for_project(
        project_root: &Path,
        fs: Arc<dyn FileSystem>,
        user_dir: Option<&Path>,
        builtin: SkillLocation,
    ) -> Self {
        let mut loader = Self::new()
            .with_location(
                SkillLocation::new(
                    Level::Project,
                    project_root.join(PROJECT_SKILLS_DIR),
                    fs.clone(),
                )
                .with_label(PROJECT_SKILLS_DIR),
            )
            .with_location(
                SkillLocation::new(
                    Level::Agentos,
                    project_root.join(AGENTOS_SKILLS_DIR),
                    fs.clone(),
                )
                .with_label(AGENTOS_SKILLS_DIR),
            );
        if let Some(dir) = user_dir {
            loader = loader.with_location(SkillLocation::new(Level::User, dir, fs));
        }
        loader.with_location(builtin)
    }

    /// Discovers all skills. Never fails; problems are returned as diagnostics.
    pub fn discover(&self) -> Discovery {
        let mut diagnostics = Vec::new();
        let mut by_level: BTreeMap<Level, BTreeMap<String, Vec<Candidate>>> = BTreeMap::new();
        for location in &self.locations {
            for candidate in scan(location, &mut diagnostics) {
                by_level
                    .entry(location.level)
                    .or_default()
                    .entry(candidate.name.clone())
                    .or_default()
                    .push(candidate);
            }
        }

        // name -> (winning level, usable skill or None if broken/duplicated, lower definitions)
        let mut winners: BTreeMap<String, (Option<Skill>, String, Vec<Shadowed>)> = BTreeMap::new();
        for (level, names) in by_level {
            for (name, mut candidates) in names {
                if let Some((_, _, shadowed)) = winners.get_mut(&name) {
                    shadowed.extend(candidates.iter().map(|c| Shadowed {
                        level,
                        path: c.display.clone(),
                    }));
                    continue;
                }
                if candidates.len() > 1 {
                    let paths: Vec<String> = candidates.iter().map(|c| c.display.clone()).collect();
                    diagnostics.push(Diagnostic::error(
                        codes::DUPLICATE_SKILL,
                        paths[0].clone(),
                        Some(&name),
                        format!(
                            "skill {name:?} is defined {} times at level {level}: {}",
                            paths.len(),
                            paths.join(", ")
                        ),
                    ));
                    winners.insert(name, (None, paths.join(", "), Vec::new()));
                    continue;
                }
                let candidate = candidates.pop().expect("one candidate");
                winners.insert(name, (candidate.skill, candidate.display, Vec::new()));
            }
        }

        let mut skills = BTreeMap::new();
        for (name, (skill, display, shadowed)) in winners {
            match skill {
                Some(mut skill) => {
                    skill.set_overrides(shadowed);
                    skills.insert(name, skill);
                }
                None => {
                    for lower in shadowed {
                        diagnostics.push(Diagnostic::warning(
                            codes::SHADOWED_BY_BROKEN,
                            lower.path.clone(),
                            Some(&name),
                            format!(
                                "not used: the higher-priority definition of {name:?} ({display}) is broken; fix or remove it"
                            ),
                        ));
                    }
                }
            }
        }
        diagnostics.sort();
        diagnostics.dedup();
        Discovery {
            registry: SkillRegistry::new(skills),
            diagnostics,
        }
    }
}

fn scan(location: &SkillLocation, diagnostics: &mut Vec<Diagnostic>) -> Vec<Candidate> {
    let fs = location.fs.as_ref();
    if !fs.exists(&location.dir) {
        tracing::debug!(dir = %location.dir.display(), level = %location.level, "skills location absent");
        return Vec::new();
    }
    let entries = match fs.read_dir(&location.dir) {
        Ok(entries) => entries,
        Err(e) => {
            diagnostics.push(Diagnostic::error(
                codes::INVALID_LOCATION,
                location.label.clone(),
                None,
                format!("cannot list skills directory: {e}"),
            ));
            return Vec::new();
        }
    };
    let mut candidates = Vec::new();
    for dir in entries {
        let Some(dir_name) = dir.file_name().and_then(|n| n.to_str()).map(str::to_owned) else {
            continue;
        };
        if dir_name.starts_with('.') || !fs.is_dir(&dir) {
            continue;
        }
        let manifest_path = dir.join(SKILL_FILE);
        let display = location.display(&manifest_path);
        if !fs.exists(&manifest_path) {
            diagnostics.push(Diagnostic::warning(
                codes::MISSING_SKILL_MD,
                location.display(&dir),
                None,
                format!("directory has no {SKILL_FILE}; ignored"),
            ));
            continue;
        }
        let (name, skill) = match load_one(location, &dir, &dir_name, &display, diagnostics) {
            Ok(skill) => (skill.name().to_owned(), Some(skill)),
            Err(claimed) => (claimed, None),
        };
        candidates.push(Candidate {
            name,
            display,
            skill,
        });
    }
    candidates
}

/// Loads one skill. On failure returns the name the broken definition claims (its declared
/// `name` if readable, else its directory name), so it still takes part in duplicate detection
/// and shadowing.
fn load_one(
    location: &SkillLocation,
    dir: &Path,
    dir_name: &str,
    display: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<Skill, String> {
    let fs = location.fs.as_ref();
    let manifest_path = dir.join(SKILL_FILE);
    let yaml = fs
        .open_read(&manifest_path)
        .map_err(|e| e.to_string())
        .and_then(|mut reader| read_front_matter(&mut reader).map_err(|e| e.to_string()));
    let yaml = match yaml {
        Ok(yaml) => yaml,
        Err(message) => {
            diagnostics.push(Diagnostic::error(
                codes::FRONT_MATTER,
                display,
                Some(dir_name),
                message,
            ));
            return Err(dir_name.to_owned());
        }
    };
    let manifest = match parse_manifest(&yaml, Some(dir_name), display) {
        Ok(m) => m,
        Err(errors) => {
            let claimed = errors
                .iter()
                .find_map(|d| d.skill.clone())
                .unwrap_or_else(|| dir_name.to_owned());
            diagnostics.extend(errors);
            return Err(claimed);
        }
    };
    let mut broken = false;
    for reference in &manifest.references {
        let path = dir.join(reference);
        if !fs.exists(&path) || fs.is_dir(&path) {
            diagnostics.push(Diagnostic::error(
                codes::MISSING_REFERENCE,
                display,
                Some(&manifest.name),
                format!("declared reference {reference:?} does not exist"),
            ));
            broken = true;
        }
    }
    if manifest.eval_cases.is_empty() {
        let message = "skill declares no eval_cases";
        diagnostics.push(if location.level == Level::Builtin {
            broken = true;
            Diagnostic::error(
                codes::MISSING_EVAL_CASES,
                display,
                Some(&manifest.name),
                message,
            )
        } else {
            Diagnostic::warning(
                codes::MISSING_EVAL_CASES,
                display,
                Some(&manifest.name),
                message,
            )
        });
    }
    if broken {
        return Err(manifest.name);
    }
    Ok(Skill::new(
        manifest,
        location.level,
        manifest_path,
        display.to_owned(),
        location.fs.clone(),
    ))
}
