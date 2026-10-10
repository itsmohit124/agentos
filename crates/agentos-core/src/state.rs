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

    /// `path` relative to the project root, or `path` itself if it is outside the root.
    pub fn relative(&self, path: &Path) -> PathBuf {
        path.strip_prefix(&self.root)
            .map(Path::to_path_buf)
            .unwrap_or_else(|_| path.to_path_buf())
    }
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
}
