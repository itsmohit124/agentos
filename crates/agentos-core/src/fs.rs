//! Filesystem abstraction.
//!
//! Subsystems perform project-state I/O through [`FileSystem`] so that behaviour can be tested
//! against [`MemFs`] without touching disk. [`RealFs`] is the production implementation.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

/// Error returned by [`FileSystem`] operations.
#[derive(Debug, thiserror::Error)]
#[error("{op} {}: {source}", path.display())]
pub struct FsError {
    /// Operation that failed (e.g. `"read"`).
    pub op: &'static str,
    /// Path the operation was applied to.
    pub path: PathBuf,
    /// Underlying I/O error.
    #[source]
    pub source: io::Error,
}

impl FsError {
    fn new(op: &'static str, path: &Path, source: io::Error) -> Self {
        Self {
            op,
            path: path.to_path_buf(),
            source,
        }
    }

    /// Kind of the underlying I/O error.
    pub fn kind(&self) -> io::ErrorKind {
        self.source.kind()
    }
}

/// Minimal filesystem interface used for project-state I/O.
pub trait FileSystem: Send + Sync {
    /// Returns `true` if anything exists at `path`.
    fn exists(&self, path: &Path) -> bool;

    /// Returns `true` if `path` is a directory.
    fn is_dir(&self, path: &Path) -> bool;

    /// Creates `path` and all missing parents. Succeeds if the directory already exists.
    fn create_dir_all(&self, path: &Path) -> Result<(), FsError>;

    /// Reads a UTF-8 file.
    fn read_to_string(&self, path: &Path) -> Result<String, FsError>;

    /// Writes `contents` to `path`, replacing any existing file. The parent must exist.
    fn write(&self, path: &Path, contents: &[u8]) -> Result<(), FsError>;

    /// Writes `contents` only if nothing exists at `path`.
    ///
    /// Returns `true` if the file was created, `false` if it already existed (left untouched).
    fn write_new(&self, path: &Path, contents: &[u8]) -> Result<bool, FsError>;

    /// Verifies that files can be created in directory `dir`.
    fn check_writable(&self, dir: &Path) -> Result<(), FsError>;
}

/// [`FileSystem`] backed by the real operating-system filesystem.
#[derive(Debug, Default, Clone, Copy)]
pub struct RealFs;

impl FileSystem for RealFs {
    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn is_dir(&self, path: &Path) -> bool {
        path.is_dir()
    }

    fn create_dir_all(&self, path: &Path) -> Result<(), FsError> {
        std::fs::create_dir_all(path).map_err(|e| FsError::new("create directory", path, e))
    }

    fn read_to_string(&self, path: &Path) -> Result<String, FsError> {
        std::fs::read_to_string(path).map_err(|e| FsError::new("read", path, e))
    }

    fn write(&self, path: &Path, contents: &[u8]) -> Result<(), FsError> {
        let tmp = sibling_temp_path(path);
        let result = std::fs::write(&tmp, contents).and_then(|()| std::fs::rename(&tmp, path));
        if result.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        result.map_err(|e| FsError::new("write", path, e))
    }

    fn write_new(&self, path: &Path, contents: &[u8]) -> Result<bool, FsError> {
        match OpenOptions::new().write(true).create_new(true).open(path) {
            Ok(mut file) => {
                file.write_all(contents)
                    .and_then(|()| file.sync_all())
                    .map_err(|e| FsError::new("write", path, e))?;
                Ok(true)
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Ok(false),
            Err(e) => Err(FsError::new("create", path, e)),
        }
    }

    fn check_writable(&self, dir: &Path) -> Result<(), FsError> {
        let probe = dir.join(format!(".agentos-write-probe-{}", std::process::id()));
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&probe)
            .map_err(|e| FsError::new("write probe in", dir, e))?;
        std::fs::remove_file(&probe).map_err(|e| FsError::new("remove write probe", &probe, e))
    }
}

fn sibling_temp_path(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    path.with_file_name(format!(".{name}.tmp-{}", std::process::id()))
}

#[derive(Debug, Clone)]
enum MemNode {
    Dir,
    File(Vec<u8>),
}

#[derive(Debug, Default)]
struct MemState {
    nodes: BTreeMap<PathBuf, MemNode>,
    read_only: BTreeSet<PathBuf>,
}

/// In-memory [`FileSystem`] for tests.
///
/// Paths are normalised lexically (`.` removed, `..` resolved). Filesystem roots always exist.
#[derive(Debug, Default)]
pub struct MemFs {
    state: Mutex<MemState>,
}

impl MemFs {
    /// Creates an empty in-memory filesystem.
    pub fn new() -> Self {
        Self::default()
    }

    /// Marks `dir` as read-only: creating or writing entries directly inside it fails.
    pub fn set_read_only(&self, dir: &Path) {
        self.lock().read_only.insert(normalize(dir));
    }

    fn lock(&self) -> MutexGuard<'_, MemState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn is_fs_root(path: &Path) -> bool {
    path.parent().is_none()
}

impl MemState {
    fn is_dir(&self, path: &Path) -> bool {
        is_fs_root(path) || matches!(self.nodes.get(path), Some(MemNode::Dir))
    }

    fn check_parent_writable(&self, op: &'static str, path: &Path) -> Result<(), FsError> {
        let parent = path.parent().unwrap_or(path);
        if !self.is_dir(parent) {
            return Err(FsError::new(op, path, io::ErrorKind::NotFound.into()));
        }
        if self.read_only.contains(parent) {
            return Err(FsError::new(
                op,
                path,
                io::ErrorKind::PermissionDenied.into(),
            ));
        }
        Ok(())
    }
}

impl FileSystem for MemFs {
    fn exists(&self, path: &Path) -> bool {
        let path = normalize(path);
        is_fs_root(&path) || self.lock().nodes.contains_key(&path)
    }

    fn is_dir(&self, path: &Path) -> bool {
        self.lock().is_dir(&normalize(path))
    }

    fn create_dir_all(&self, path: &Path) -> Result<(), FsError> {
        let path = normalize(path);
        let mut state = self.lock();
        let mut missing: Vec<PathBuf> = path
            .ancestors()
            .take_while(|p| !is_fs_root(p) && !state.is_dir(p))
            .map(Path::to_path_buf)
            .collect();
        missing.reverse();
        for dir in missing {
            if state.nodes.contains_key(&dir) {
                return Err(FsError::new(
                    "create directory",
                    &dir,
                    io::ErrorKind::AlreadyExists.into(),
                ));
            }
            state.check_parent_writable("create directory", &dir)?;
            state.nodes.insert(dir, MemNode::Dir);
        }
        Ok(())
    }

    fn read_to_string(&self, path: &Path) -> Result<String, FsError> {
        let path = normalize(path);
        match self.lock().nodes.get(&path) {
            Some(MemNode::File(bytes)) => String::from_utf8(bytes.clone()).map_err(|e| {
                FsError::new("read", &path, io::Error::new(io::ErrorKind::InvalidData, e))
            }),
            Some(MemNode::Dir) => Err(FsError::new(
                "read",
                &path,
                io::ErrorKind::IsADirectory.into(),
            )),
            None => Err(FsError::new("read", &path, io::ErrorKind::NotFound.into())),
        }
    }

    fn write(&self, path: &Path, contents: &[u8]) -> Result<(), FsError> {
        let path = normalize(path);
        let mut state = self.lock();
        state.check_parent_writable("write", &path)?;
        if matches!(state.nodes.get(&path), Some(MemNode::Dir)) {
            return Err(FsError::new(
                "write",
                &path,
                io::ErrorKind::IsADirectory.into(),
            ));
        }
        state.nodes.insert(path, MemNode::File(contents.to_vec()));
        Ok(())
    }

    fn write_new(&self, path: &Path, contents: &[u8]) -> Result<bool, FsError> {
        let path = normalize(path);
        let mut state = self.lock();
        if state.nodes.contains_key(&path) {
            return Ok(false);
        }
        state.check_parent_writable("create", &path)?;
        state.nodes.insert(path, MemNode::File(contents.to_vec()));
        Ok(true)
    }

    fn check_writable(&self, dir: &Path) -> Result<(), FsError> {
        let dir = normalize(dir);
        let state = self.lock();
        if !state.is_dir(&dir) {
            return Err(FsError::new(
                "write probe in",
                &dir,
                io::ErrorKind::NotFound.into(),
            ));
        }
        if state.read_only.contains(&dir) {
            return Err(FsError::new(
                "write probe in",
                &dir,
                io::ErrorKind::PermissionDenied.into(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Contract shared by every [`FileSystem`] implementation.
    fn contract(fs: &dyn FileSystem, base: &Path) {
        let dir = base.join("a/b");
        assert!(!fs.exists(&dir));
        fs.create_dir_all(&dir).unwrap();
        fs.create_dir_all(&dir).unwrap();
        assert!(fs.is_dir(&dir));

        let file = dir.join("f.txt");
        assert!(fs.write_new(&file, b"one").unwrap());
        assert!(!fs.write_new(&file, b"two").unwrap());
        assert_eq!(fs.read_to_string(&file).unwrap(), "one");

        fs.write(&file, b"three").unwrap();
        assert_eq!(fs.read_to_string(&file).unwrap(), "three");
        assert!(fs.exists(&file));
        assert!(!fs.is_dir(&file));

        let missing = base.join("missing.txt");
        assert_eq!(
            fs.read_to_string(&missing).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
        assert!(fs.write(&base.join("no/parent.txt"), b"x").is_err());

        fs.check_writable(&dir).unwrap();
        assert!(fs.check_writable(&base.join("nope")).is_err());
    }

    #[test]
    fn real_fs_satisfies_contract() {
        let tmp = tempfile::tempdir().unwrap();
        contract(&RealFs, tmp.path());
    }

    #[test]
    fn real_fs_check_writable_leaves_no_probe() {
        let tmp = tempfile::tempdir().unwrap();
        RealFs.check_writable(tmp.path()).unwrap();
        assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 0);
    }

    #[test]
    fn mem_fs_satisfies_contract() {
        contract(&MemFs::new(), Path::new("/project"));
    }

    #[test]
    fn mem_fs_normalizes_paths() {
        let fs = MemFs::new();
        fs.create_dir_all(Path::new("/p/x")).unwrap();
        fs.write(Path::new("/p/./x/../x/f"), b"v").unwrap();
        assert_eq!(fs.read_to_string(Path::new("/p/x/f")).unwrap(), "v");
    }

    #[test]
    fn mem_fs_read_only_dir_rejects_writes() {
        let fs = MemFs::new();
        fs.create_dir_all(Path::new("/ro")).unwrap();
        fs.set_read_only(Path::new("/ro"));
        let err = fs.write_new(Path::new("/ro/f"), b"x").unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
        assert!(fs.check_writable(Path::new("/ro")).is_err());
    }

    #[test]
    fn mem_fs_create_dir_over_file_fails() {
        let fs = MemFs::new();
        fs.create_dir_all(Path::new("/p")).unwrap();
        fs.write(Path::new("/p/file"), b"x").unwrap();
        assert!(fs.create_dir_all(Path::new("/p/file/sub")).is_err());
    }
}
