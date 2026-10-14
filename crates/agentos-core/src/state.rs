//! The `.agentos/` project-state directory.
//!
//! [`StateLayout`] names every path inside `.agentos/`; [`StateLayout::ensure`] creates the
//! directory and its seed files idempotently, never overwriting existing content.

use std::path::{Path, PathBuf};

use crate::fs::{FileSystem, FsError};

/// Name of the project-state directory under the project root.
pub const STATE_DIR: &str = ".agentos";
/// File name of the project configuration inside [`STATE_DIR`].
pub const CONFIG_FILE: &str = "config.toml";
/// File name of the derived SQLite index inside [`STATE_DIR`].
pub const DB_FILE: &str = "agentos.db";

/// Ignore line for adapter backups of pre-existing user files.
///
/// The pattern is relative to `.agentos/`.
pub const ORIGINALS_IGNORE_LINE: &str = "adapters/originals/";

/// Contents of `.agentos/.gitignore`: the SQLite index is derived state and is not committed.
const STATE_GITIGNORE: &str = "\
# Derived state, rebuildable from the project (do not commit).
agentos.db
agentos.db-journal
agentos.db-wal
agentos.db-shm
project-profile.json
context-cache/
feedback/
runs/
adapters/originals/
";

/// Error returned by [`StateLayout::ensure`].
#[derive(Debug, thiserror::Error)]
pub enum StateError {
    /// Something other than a directory exists at the state-directory path.
    #[error("{} exists but is not a directory", .0.display())]
    NotADirectory(PathBuf),
    /// A filesystem operation failed.
    #[error(transparent)]
    Fs(#[from] FsError),
}

/// Whether [`StateLayout::ensure`] created an entry or found it already present.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryStatus {
    /// The entry was created by this call.
    Created,
    /// The entry already existed and was left untouched.
    Existing,
}

impl EntryStatus {
    /// Stable machine-readable name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Existing => "exists",
        }
    }
}

/// One entry handled by [`StateLayout::ensure`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnsuredEntry {
    /// Path relative to the project root (e.g. `.agentos/config.toml`).
    pub relative_path: PathBuf,
    /// Whether it was created or already existed.
    pub status: EntryStatus,
}

/// Paths of the project-state directory for one project root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateLayout {
    root: PathBuf,
}

impl StateLayout {
    /// Layout for the project rooted at `root`.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Project root.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// `<root>/.agentos`.
    pub fn dir(&self) -> PathBuf {
        self.root.join(STATE_DIR)
    }

    /// `<root>/.agentos/config.toml`.
    pub fn config_file(&self) -> PathBuf {
        self.dir().join(CONFIG_FILE)
    }

    /// `<root>/.agentos/agentos.db`.
    pub fn db_file(&self) -> PathBuf {
        self.dir().join(DB_FILE)
    }

    /// `<root>/.agentos/.gitignore`.
    pub fn gitignore_file(&self) -> PathBuf {
        self.dir().join(".gitignore")
    }

    /// Creates `.agentos/`, `config.toml` (with `default_config`) and `.gitignore` if missing.
    ///
    /// Existing entries are never modified, so calling this repeatedly is safe.
    /// Returns the entries in a fixed order.
    pub fn ensure(
        &self,
        fs: &dyn FileSystem,
        default_config: &str,
    ) -> Result<Vec<EnsuredEntry>, StateError> {
        let dir = self.dir();
        let dir_status = if fs.is_dir(&dir) {
            EntryStatus::Existing
        } else if fs.exists(&dir) {
            return Err(StateError::NotADirectory(dir));
        } else {
            fs.create_dir_all(&dir)?;
            EntryStatus::Created
        };

        let mut entries = vec![EnsuredEntry {
            relative_path: PathBuf::from(STATE_DIR),
            status: dir_status,
        }];
        for (path, contents) in [
            (self.config_file(), default_config),
            (self.gitignore_file(), STATE_GITIGNORE),
        ] {
            let status = if fs.write_new(&path, contents.as_bytes())? {
                EntryStatus::Created
            } else {
                EntryStatus::Existing
            };
            entries.push(EnsuredEntry {
                relative_path: self.relative(&path),
                status,
            });
        }
        Ok(entries)
    }

    /// Makes sure `.agentos/.gitignore` ignores [`ORIGINALS_IGNORE_LINE`].
    ///
    /// A missing file is created with the standard derived-state ignore list. An existing file
    /// keeps every current byte. The ignore line is appended only when no active line already
    /// names that directory. Returns `true` when the file was written.
    pub fn ensure_originals_ignored(&self, fs: &dyn FileSystem) -> Result<bool, StateError> {
        let dir = self.dir();
        if !fs.is_dir(&dir) {
            if fs.exists(&dir) {
                return Err(StateError::NotADirectory(dir));
            }
            fs.create_dir_all(&dir)?;
        }
        let path = self.gitignore_file();
        if !fs.exists(&path) {
            fs.write(&path, STATE_GITIGNORE.as_bytes())?;
            return Ok(true);
        }
        let current = fs.read_to_string(&path)?;
        let Some(updated) = merge_gitignore_line(&current, ORIGINALS_IGNORE_LINE) else {
            return Ok(false);
        };
        fs.write(&path, updated.as_bytes())?;
        Ok(true)
    }

    /// `path` relative to the project root, or `path` itself if it is outside the root.
    pub fn relative(&self, path: &Path) -> PathBuf {
        path.strip_prefix(&self.root)
            .map(Path::to_path_buf)
            .unwrap_or_else(|_| path.to_path_buf())
    }
}

/// Appends `line` when no active gitignore line already names that path.
///
/// `None` means `existing` already covers it and must be left byte for byte.
pub fn merge_gitignore_line(existing: &str, line: &str) -> Option<String> {
    if gitignore_covers(existing, line) {
        return None;
    }
    let mut out = existing.to_owned();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(line.trim_end_matches(['\n', '\r']));
    out.push('\n');
    Some(out)
}

fn gitignore_covers(existing: &str, line: &str) -> bool {
    let wanted = normalize_ignore(line);
    if wanted.is_empty() {
        return true;
    }
    existing.lines().any(|raw| normalize_ignore(raw) == wanted)
}

fn normalize_ignore(line: &str) -> String {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return String::new();
    }
    trimmed.trim_end_matches('/').to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::MemFs;

    fn setup() -> (MemFs, StateLayout) {
        let fs = MemFs::new();
        fs.create_dir_all(Path::new("/proj")).unwrap();
        (fs, StateLayout::new("/proj"))
    }

    fn statuses(entries: &[EnsuredEntry]) -> Vec<(String, EntryStatus)> {
        entries
            .iter()
            .map(|e| (e.relative_path.display().to_string(), e.status))
            .collect()
    }

    #[test]
    fn layout_paths() {
        let layout = StateLayout::new("/proj");
        assert_eq!(layout.dir(), PathBuf::from("/proj/.agentos"));
        assert_eq!(
            layout.config_file(),
            PathBuf::from("/proj/.agentos/config.toml")
        );
        assert_eq!(layout.db_file(), PathBuf::from("/proj/.agentos/agentos.db"));
    }

    #[test]
    fn ensure_creates_then_is_idempotent() {
        let (fs, layout) = setup();
        let first = layout.ensure(&fs, "x = 1\n").unwrap();
        assert_eq!(
            statuses(&first),
            vec![
                (".agentos".into(), EntryStatus::Created),
                (".agentos/config.toml".into(), EntryStatus::Created),
                (".agentos/.gitignore".into(), EntryStatus::Created),
            ]
        );
        let second = layout.ensure(&fs, "x = 1\n").unwrap();
        assert!(second.iter().all(|e| e.status == EntryStatus::Existing));
    }

    #[test]
    fn ensure_never_overwrites_user_config() {
        let (fs, layout) = setup();
        layout.ensure(&fs, "default\n").unwrap();
        fs.write(&layout.config_file(), b"user edited\n").unwrap();
        layout.ensure(&fs, "default\n").unwrap();
        assert_eq!(
            fs.read_to_string(&layout.config_file()).unwrap(),
            "user edited\n"
        );
    }

    #[test]
    fn ensure_restores_missing_seed_file_only() {
        let (fs, layout) = setup();
        fs.create_dir_all(&layout.dir()).unwrap();
        fs.write(&layout.config_file(), b"mine\n").unwrap();
        let entries = layout.ensure(&fs, "default\n").unwrap();
        assert_eq!(
            statuses(&entries),
            vec![
                (".agentos".into(), EntryStatus::Existing),
                (".agentos/config.toml".into(), EntryStatus::Existing),
                (".agentos/.gitignore".into(), EntryStatus::Created),
            ]
        );
    }

    #[test]
    fn ensure_rejects_file_at_state_dir() {
        let (fs, layout) = setup();
        fs.write(&layout.dir(), b"oops").unwrap();
        assert!(matches!(
            layout.ensure(&fs, ""),
            Err(StateError::NotADirectory(_))
        ));
    }

    #[test]
    fn ensure_reports_unwritable_root() {
        let (fs, layout) = setup();
        fs.set_read_only(Path::new("/proj"));
        assert!(matches!(layout.ensure(&fs, ""), Err(StateError::Fs(_))));
    }

    #[test]
    fn new_gitignore_ignores_adapter_originals() {
        let (fs, layout) = setup();
        layout.ensure(&fs, "x = 1\n").unwrap();
        let text = fs.read_to_string(&layout.gitignore_file()).unwrap();
        assert!(text.contains("adapters/originals/\n"));
        assert!(!layout.ensure_originals_ignored(&fs).unwrap());
        assert_eq!(fs.read_to_string(&layout.gitignore_file()).unwrap(), text);
    }

    #[test]
    fn existing_gitignore_keeps_user_bytes_and_gains_one_line() {
        let (fs, layout) = setup();
        fs.create_dir_all(&layout.dir()).unwrap();
        let original = "keep-me\n# adapters/originals/\n";
        fs.write(&layout.gitignore_file(), original.as_bytes())
            .unwrap();
        assert!(layout.ensure_originals_ignored(&fs).unwrap());
        let text = fs.read_to_string(&layout.gitignore_file()).unwrap();
        assert_eq!(
            text,
            "keep-me\n# adapters/originals/\nadapters/originals/\n"
        );
        assert!(!layout.ensure_originals_ignored(&fs).unwrap());
        assert_eq!(fs.read_to_string(&layout.gitignore_file()).unwrap(), text);
    }

    #[test]
    fn an_ignore_line_without_a_trailing_slash_is_enough() {
        let (fs, layout) = setup();
        fs.create_dir_all(&layout.dir()).unwrap();
        let original = "custom\nadapters/originals\n";
        fs.write(&layout.gitignore_file(), original.as_bytes())
            .unwrap();
        assert!(!layout.ensure_originals_ignored(&fs).unwrap());
        assert_eq!(
            fs.read_to_string(&layout.gitignore_file()).unwrap(),
            original
        );
    }
}
