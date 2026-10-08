//! Built-in skills, compiled into the crate from the workspace `skills/` directory.

use std::path::Path;
use std::sync::Arc;

use agentos_core::fs::{FileSystem, MemFs};

use crate::loader::{Level, SkillLocation};

include!(concat!(env!("OUT_DIR"), "/builtin_skills.rs"));

/// Virtual directory the built-in skills are served from.
pub const BUILTIN_DIR: &str = "/agentos-builtin/skills";

/// Display prefix of built-in skill paths (e.g. `builtin:tdd/SKILL.md`).
pub const BUILTIN_LABEL: &str = "builtin:";

/// Paths of the embedded built-in files, relative to `skills/`, sorted.
pub fn files() -> impl Iterator<Item = &'static str> {
    FILES.iter().map(|(path, _)| *path)
}

/// The built-in skills as a [`Level::Builtin`] location backed by an in-memory filesystem.
pub fn location() -> SkillLocation {
    let fs = MemFs::new();
    let root = Path::new(BUILTIN_DIR);
    for (relative, bytes) in FILES {
        let path = root.join(relative);
        let parent = path.parent().expect("built-in file has a parent");
        fs.create_dir_all(parent)
            .and_then(|()| fs.write(&path, bytes))
            .expect("in-memory filesystem accepts built-in files");
    }
    if FILES.is_empty() {
        fs.create_dir_all(root).expect("create built-in root");
    }
    SkillLocation::new(Level::Builtin, root, Arc::new(fs)).with_label(BUILTIN_LABEL)
}
