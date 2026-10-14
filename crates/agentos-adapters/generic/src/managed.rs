//! Install and uninstall of AgentOS-owned file sections.
//!
//! Markdown files use the markers `<!-- agentos:begin -->` and `<!-- agentos:end -->`.
//! The manifest under `.agentos/adapters/manifest.json` records which adapter created a
//! file and, for a pre-existing file, a copy of the original bytes. Uninstall removes
//! only that adapter's block. When the bytes outside the block still match the saved
//! original, the original file is restored byte for byte.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::AdapterError;
use crate::types::{
    FileAction, FileActionKind, InstallReport, ManagedWrite, UninstallReport, WritePolicy,
};

const BEGIN: &str = "<!-- agentos:begin -->";
const END: &str = "<!-- agentos:end -->";
const MANIFEST_REL: &str = ".agentos/adapters/manifest.json";
const VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    version: u32,
    files: BTreeMap<String, FileRecord>,
    dirs: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileRecord {
    adapter: String,
    created: bool,
    policy: String,
    separator: String,
    prelude: String,
    original: Option<String>,
    generated: Option<String>,
}

impl Default for Manifest {
    fn default() -> Self {
        Self {
            version: VERSION,
            files: BTreeMap::new(),
            dirs: BTreeMap::new(),
        }
    }
}

/// Installs `writes` for `adapter`. A second call with the same bytes reports `unchanged`.
pub fn install(
    root: &Path,
    adapter: &str,
    writes: &[ManagedWrite],
) -> Result<InstallReport, AdapterError> {
    ensure_project(root)?;
    ensure_backup_ignored(root)?;
    crate::native::seal(root)?;
    let mut manifest = load_manifest(root)?;
    let mut actions = Vec::new();
    let planned: BTreeSet<String> = writes.iter().map(|write| write.path.clone()).collect();
    for write in writes {
        let action = apply_one(root, adapter, write, &mut manifest)?;
        actions.push(action);
        save_manifest(root, &manifest)?;
    }
    let stale: Vec<String> = manifest
        .files
        .iter()
        .filter(|(path, record)| record.adapter == adapter && !planned.contains(*path))
        .map(|(path, _)| path.clone())
        .collect();
    for path in stale {
        actions.push(remove_record(root, adapter, &path, &mut manifest)?);
        save_manifest(root, &manifest)?;
    }
    cleanup_dirs(root, &mut manifest)?;
    save_manifest(root, &manifest)?;
    actions.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(InstallReport {
        adapter: adapter.to_owned(),
        files: actions,
    })
}

/// Removes managed content for `adapter`. User bytes outside a managed block stay.
pub fn uninstall(root: &Path, adapter: &str) -> Result<UninstallReport, AdapterError> {
    ensure_project(root)?;
    let mut manifest = load_manifest(root)?;
    let paths: Vec<String> = manifest
        .files
        .iter()
        .filter(|(_, record)| record.adapter == adapter)
        .map(|(path, _)| path.clone())
        .collect();
    let mut actions = Vec::new();
    if paths.is_empty() {
        actions.push(FileAction {
            path: String::new(),
            action: FileActionKind::Unchanged,
            detail: "nothing installed".to_owned(),
        });
    }
    for path in paths {
        actions.push(remove_record(root, adapter, &path, &mut manifest)?);
        save_manifest(root, &manifest)?;
    }
    cleanup_dirs(root, &mut manifest)?;
    save_manifest(root, &manifest)?;
    actions.retain(|action| !action.path.is_empty());
    actions.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(UninstallReport {
        adapter: adapter.to_owned(),
        files: actions,
    })
}

/// True when `adapter` has at least one manifest record.
pub fn is_installed(root: &Path, adapter: &str) -> Result<bool, AdapterError> {
    let manifest = load_manifest(root)?;
    Ok(manifest
        .files
        .values()
        .any(|record| record.adapter == adapter))
}

/// Relative paths currently recorded for `adapter`, sorted.
pub fn installed_paths(root: &Path, adapter: &str) -> Result<Vec<String>, AdapterError> {
    let manifest = load_manifest(root)?;
    Ok(manifest
        .files
        .iter()
        .filter(|(_, record)| record.adapter == adapter)
        .map(|(path, _)| path.clone())
        .collect())
}

fn apply_one(
    root: &Path,
    adapter: &str,
    write: &ManagedWrite,
    manifest: &mut Manifest,
) -> Result<FileAction, AdapterError> {
    let rel = validate_rel(&write.path)?;
    match &write.policy {
        WritePolicy::Markers { prelude, body } => {
            apply_markers(root, adapter, &write.path, &rel, prelude, body, manifest)
        }
        WritePolicy::Whole { contents } => {
            apply_whole(root, adapter, &write.path, &rel, contents, manifest)
        }
    }
}

fn apply_markers(
    root: &Path,
    adapter: &str,
    key: &str,
    rel: &Path,
    prelude: &str,
    body: &str,
    manifest: &mut Manifest,
) -> Result<FileAction, AdapterError> {
    let block = render_block(body).map_err(|message| AdapterError::managed(key, message))?;
    let abs = root.join(rel);
    if let Some(record) = manifest.files.get(key) {
        if record.adapter != adapter {
            return Err(AdapterError::managed(
                key,
                format!("managed block is owned by adapter {}", record.adapter),
            ));
        }
        if record.policy != "markers" {
            return Err(AdapterError::managed(key, "file is not marker-owned"));
        }
        let text = read_utf8(&abs).map_err(|err| annotate(key, err))?;
        let split = split_block(&text).map_err(|message| AdapterError::managed(key, message))?;
        let new_text = format!("{}{block}{}", split.before, split.after);
        if new_text == text {
            return Ok(action(key, FileActionKind::Unchanged, String::new()));
        }
        write_atomic(&abs, new_text.as_bytes()).map_err(|err| annotate(key, err))?;
        return Ok(action(key, FileActionKind::Updated, String::new()));
    }
    if abs.exists() {
        if abs.is_dir() {
            return Err(AdapterError::managed(key, "path is a directory"));
        }
        let text = read_utf8(&abs).map_err(|err| annotate(key, err))?;
        if text.contains(BEGIN) || text.contains(END) {
            return Err(AdapterError::managed(
                key,
                "ambiguous agentos markers; file was not modified",
            ));
        }
        let separator = if text.is_empty() || text.ends_with('\n') {
            String::new()
        } else {
            "\n".to_owned()
        };
        let original = save_bytes(root, adapter, "originals", key, text.as_bytes())?;
        remember_dirs(root, rel, adapter, manifest)?;
        let new_text = format!("{text}{separator}{block}");
        write_atomic(&abs, new_text.as_bytes()).map_err(|err| annotate(key, err))?;
        manifest.files.insert(
            key.to_owned(),
            FileRecord {
                adapter: adapter.to_owned(),
                created: false,
                policy: "markers".to_owned(),
                separator,
                prelude: String::new(),
                original: Some(original),
                generated: None,
            },
        );
        return Ok(action(
            key,
            FileActionKind::Updated,
            "section added".to_owned(),
        ));
    }
    let prelude = normalize_prelude(prelude);
    remember_dirs(root, rel, adapter, manifest)?;
    let new_text = format!("{prelude}{block}");
    write_atomic(&abs, new_text.as_bytes()).map_err(|err| annotate(key, err))?;
    manifest.files.insert(
        key.to_owned(),
        FileRecord {
            adapter: adapter.to_owned(),
            created: true,
            policy: "markers".to_owned(),
            separator: String::new(),
            prelude,
            original: None,
            generated: None,
        },
    );
    Ok(action(key, FileActionKind::Created, String::new()))
}

fn apply_whole(
    root: &Path,
    adapter: &str,
    key: &str,
    rel: &Path,
    contents: &str,
    manifest: &mut Manifest,
) -> Result<FileAction, AdapterError> {
    let abs = root.join(rel);
    if let Some(record) = manifest.files.get(key).cloned() {
        if record.adapter != adapter {
            return Err(AdapterError::managed(
                key,
                format!("file is owned by adapter {}", record.adapter),
            ));
        }
        if record.policy != "whole" {
            return Err(AdapterError::managed(key, "file is not wholly owned"));
        }
        if !abs.is_file() {
            return Err(AdapterError::managed(key, "recorded file is missing"));
        }
        let current = read_utf8(&abs).map_err(|err| annotate(key, err))?;
        if current == contents {
            return Ok(action(key, FileActionKind::Unchanged, String::new()));
        }
        let previous = match &record.generated {
            Some(path) => read_utf8(&root.join(path)).unwrap_or_default(),
            None => String::new(),
        };
        if current != previous {
            return Ok(action(
                key,
                FileActionKind::Retained,
                "user modified the file".to_owned(),
            ));
        }
        write_atomic(&abs, contents.as_bytes()).map_err(|err| annotate(key, err))?;
        let generated = save_bytes(root, adapter, "generated", key, contents.as_bytes())?;
        if let Some(slot) = manifest.files.get_mut(key) {
            slot.generated = Some(generated);
        }
        return Ok(action(key, FileActionKind::Updated, String::new()));
    }
    if abs.exists() {
        return Ok(action(
            key,
            FileActionKind::Retained,
            "pre-existing file was not overwritten".to_owned(),
        ));
    }
    remember_dirs(root, rel, adapter, manifest)?;
    write_atomic(&abs, contents.as_bytes()).map_err(|err| annotate(key, err))?;
    let generated = save_bytes(root, adapter, "generated", key, contents.as_bytes())?;
    manifest.files.insert(
        key.to_owned(),
        FileRecord {
            adapter: adapter.to_owned(),
            created: true,
            policy: "whole".to_owned(),
            separator: String::new(),
            prelude: String::new(),
            original: None,
            generated: Some(generated),
        },
    );
    Ok(action(key, FileActionKind::Created, String::new()))
}

fn remove_record(
    root: &Path,
    adapter: &str,
    key: &str,
    manifest: &mut Manifest,
) -> Result<FileAction, AdapterError> {
    let Some(record) = manifest.files.get(key).cloned() else {
        return Ok(action(
            key,
            FileActionKind::Unchanged,
            "not recorded".to_owned(),
        ));
    };
    if record.adapter != adapter {
        return Ok(action(
            key,
            FileActionKind::Retained,
            format!("owned by adapter {}", record.adapter),
        ));
    }
    let rel = validate_rel(key)?;
    let abs = root.join(&rel);
    let action = if record.policy == "whole" {
        remove_whole(root, key, &abs, &record)?
    } else {
        remove_markers(root, key, &abs, &record)?
    };
    if action.action != FileActionKind::Failed && action.action != FileActionKind::Retained {
        manifest.files.remove(key);
        if let Some(path) = &record.original {
            let _ = fs::remove_file(root.join(path));
        }
        if let Some(path) = &record.generated {
            let _ = fs::remove_file(root.join(path));
        }
    }
    Ok(action)
}

fn remove_markers(
    root: &Path,
    key: &str,
    abs: &Path,
    record: &FileRecord,
) -> Result<FileAction, AdapterError> {
    if !abs.exists() {
        return Ok(action(
            key,
            FileActionKind::Removed,
            "already absent".to_owned(),
        ));
    }
    let text = match read_utf8(abs) {
        Ok(text) => text,
        Err(err) => {
            return Ok(action(key, FileActionKind::Failed, err.to_string()));
        }
    };
    let split = match split_block(&text) {
        Ok(split) => split,
        Err(message) => return Ok(action(key, FileActionKind::Failed, message)),
    };
    if record.created && split.before == record.prelude && split.after.is_empty() {
        fs::remove_file(abs).map_err(|err| annotate(key, AdapterError::io(abs, &err)))?;
        return Ok(action(
            key,
            FileActionKind::Removed,
            "created file deleted".to_owned(),
        ));
    }
    let outside = outside_bytes(&split.before, &split.after, &record.separator);
    if let Some(original_rel) = &record.original
        && let Ok(original) = fs::read(root.join(original_rel))
        && outside.as_bytes() == original.as_slice()
    {
        write_atomic(abs, &original).map_err(|err| annotate(key, err))?;
        return Ok(action(
            key,
            FileActionKind::Removed,
            "original bytes restored".to_owned(),
        ));
    }
    if outside.is_empty() && record.created {
        fs::remove_file(abs).map_err(|err| annotate(key, AdapterError::io(abs, &err)))?;
        return Ok(action(
            key,
            FileActionKind::Removed,
            "created file deleted".to_owned(),
        ));
    }
    write_atomic(abs, outside.as_bytes()).map_err(|err| annotate(key, err))?;
    Ok(action(
        key,
        FileActionKind::Removed,
        "user content retained".to_owned(),
    ))
}

fn remove_whole(
    root: &Path,
    key: &str,
    abs: &Path,
    record: &FileRecord,
) -> Result<FileAction, AdapterError> {
    if !record.created {
        return Ok(action(
            key,
            FileActionKind::Retained,
            "pre-existing file kept".to_owned(),
        ));
    }
    if !abs.exists() {
        return Ok(action(
            key,
            FileActionKind::Removed,
            "already absent".to_owned(),
        ));
    }
    let current = match fs::read(abs) {
        Ok(bytes) => bytes,
        Err(err) => return Ok(action(key, FileActionKind::Failed, err.to_string())),
    };
    let previous = record
        .generated
        .as_ref()
        .and_then(|path| fs::read(root.join(path)).ok())
        .unwrap_or_default();
    if current == previous {
        fs::remove_file(abs).map_err(|err| annotate(key, AdapterError::io(abs, &err)))?;
        return Ok(action(
            key,
            FileActionKind::Removed,
            "created file deleted".to_owned(),
        ));
    }
    Ok(action(
        key,
        FileActionKind::Retained,
        "user modified the file".to_owned(),
    ))
}

fn outside_bytes(before: &str, after: &str, separator: &str) -> String {
    let mut prefix = before.to_owned();
    if after.is_empty() && !separator.is_empty() && prefix.ends_with(separator) {
        prefix.truncate(prefix.len() - separator.len());
    }
    format!("{prefix}{after}")
}

struct Split {
    before: String,
    after: String,
}

fn split_block(text: &str) -> Result<Split, String> {
    let begins = text.matches(BEGIN).count();
    let ends = text.matches(END).count();
    if begins == 0 && ends == 0 {
        return Err("managed markers are missing; file was not modified".to_owned());
    }
    if begins != 1 || ends != 1 {
        return Err("malformed or duplicate agentos markers; file was not modified".to_owned());
    }
    let begin_at = text.find(BEGIN).expect("begin count is 1");
    let end_at = text.find(END).expect("end count is 1");
    if end_at < begin_at + BEGIN.len() {
        return Err(
            "agentos end marker appears before the begin marker; file was not modified".to_owned(),
        );
    }
    let mut end_exclusive = end_at + END.len();
    if text[end_exclusive..].starts_with('\n') {
        end_exclusive += 1;
    }
    Ok(Split {
        before: text[..begin_at].to_owned(),
        after: text[end_exclusive..].to_owned(),
    })
}

fn render_block(body: &str) -> Result<String, String> {
    if body.contains(BEGIN) || body.contains(END) {
        return Err("managed body contains an agentos marker".to_owned());
    }
    let mut body = body.to_owned();
    if !body.is_empty() && !body.ends_with('\n') {
        body.push('\n');
    }
    Ok(format!("{BEGIN}\n{body}{END}\n"))
}

fn normalize_prelude(prelude: &str) -> String {
    if prelude.is_empty() || prelude.ends_with('\n') {
        prelude.to_owned()
    } else {
        format!("{prelude}\n")
    }
}

fn remember_dirs(
    root: &Path,
    rel: &Path,
    adapter: &str,
    manifest: &mut Manifest,
) -> Result<(), AdapterError> {
    let abs = root.join(rel);
    let parent = abs.parent().unwrap_or(root);
    let mut missing = Vec::new();
    let mut cursor = parent.to_path_buf();
    while cursor.starts_with(root) && cursor != root && !cursor.exists() {
        missing.push(cursor.clone());
        let Some(next) = cursor.parent() else {
            break;
        };
        cursor = next.to_path_buf();
    }
    fs::create_dir_all(parent).map_err(|err| AdapterError::io(parent, &err))?;
    for dir in missing {
        let relative = dir
            .strip_prefix(root)
            .unwrap_or(&dir)
            .components()
            .map(|component| component.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/");
        manifest
            .dirs
            .entry(relative)
            .or_insert_with(|| adapter.to_owned());
    }
    Ok(())
}

fn cleanup_dirs(root: &Path, manifest: &mut Manifest) -> Result<(), AdapterError> {
    let mut dirs: Vec<String> = manifest.dirs.keys().cloned().collect();
    dirs.sort_by_key(|path| std::cmp::Reverse(path.matches('/').count()));
    for dir in dirs {
        let abs = root.join(&dir);
        let empty = abs.is_dir()
            && fs::read_dir(&abs)
                .map(|mut entries| entries.next().is_none())
                .unwrap_or(false);
        if empty {
            fs::remove_dir(&abs).map_err(|err| AdapterError::io(&abs, &err))?;
            manifest.dirs.remove(&dir);
        }
    }
    if manifest.files.is_empty() && manifest.dirs.is_empty() {
        let manifest_path = root.join(MANIFEST_REL);
        if manifest_path.is_file() {
            let _ = fs::remove_file(&manifest_path);
        }
        let adapters = root.join(".agentos/adapters");
        if adapters.is_dir()
            && fs::read_dir(&adapters)
                .map(|mut entries| entries.next().is_none())
                .unwrap_or(false)
        {
            let _ = fs::remove_dir(&adapters);
        }
    }
    Ok(())
}

/// True when `.agentos/.gitignore` already ignores adapter backup copies.
pub fn originals_ignored(root: &Path) -> bool {
    fs::read_to_string(root.join(".agentos/.gitignore"))
        .ok()
        .is_some_and(|text| {
            agentos_core::state::merge_gitignore_line(
                &text,
                agentos_core::state::ORIGINALS_IGNORE_LINE,
            )
            .is_none()
        })
}

fn ensure_backup_ignored(root: &Path) -> Result<(), AdapterError> {
    let layout = agentos_core::state::StateLayout::new(root);
    layout
        .ensure_originals_ignored(&agentos_core::fs::RealFs)
        .map_err(|err| AdapterError::Io {
            path: ".agentos/.gitignore".to_owned(),
            message: err.to_string(),
        })?;
    Ok(())
}

fn save_bytes(
    root: &Path,
    adapter: &str,
    bucket: &str,
    key: &str,
    bytes: &[u8],
) -> Result<String, AdapterError> {
    let relative = format!(".agentos/adapters/{bucket}/{adapter}/{key}");
    let path = root.join(&relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| AdapterError::io(parent, &err))?;
    }
    write_atomic(&path, bytes)?;
    Ok(relative)
}

fn load_manifest(root: &Path) -> Result<Manifest, AdapterError> {
    let path = root.join(MANIFEST_REL);
    if !path.exists() {
        return Ok(Manifest::default());
    }
    let text = read_utf8(&path)?;
    serde_json::from_str(&text).map_err(|err| AdapterError::managed(MANIFEST_REL, err.to_string()))
}

fn save_manifest(root: &Path, manifest: &Manifest) -> Result<(), AdapterError> {
    if manifest.files.is_empty() && manifest.dirs.is_empty() {
        let path = root.join(MANIFEST_REL);
        if path.is_file() {
            let _ = fs::remove_file(&path);
        }
        return Ok(());
    }
    let path = root.join(MANIFEST_REL);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| AdapterError::io(parent, &err))?;
    }
    let mut text = serde_json::to_string_pretty(manifest)
        .map_err(|err| AdapterError::managed(MANIFEST_REL, err.to_string()))?;
    text.push('\n');
    write_atomic(&path, text.as_bytes())
}

fn ensure_project(root: &Path) -> Result<(), AdapterError> {
    if root.is_dir() {
        Ok(())
    } else {
        Err(AdapterError::NotAProject {
            path: root.to_path_buf(),
        })
    }
}

fn validate_rel(path: &str) -> Result<PathBuf, AdapterError> {
    if path.is_empty() || path.starts_with('/') || path.contains('\\') || path.contains('\0') {
        return Err(AdapterError::UnsafePath {
            path: path.to_owned(),
        });
    }
    let parsed = Path::new(path);
    if parsed.is_absolute()
        || parsed.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(AdapterError::UnsafePath {
            path: path.to_owned(),
        });
    }
    Ok(parsed.to_path_buf())
}

fn read_utf8(path: &Path) -> Result<String, AdapterError> {
    let bytes = fs::read(path).map_err(|err| AdapterError::io(path, &err))?;
    String::from_utf8(bytes).map_err(|_| {
        AdapterError::managed(
            &path.display().to_string(),
            "file is not valid UTF-8; it was not modified",
        )
    })
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), AdapterError> {
    let mut tmp_name = path
        .file_name()
        .ok_or_else(|| AdapterError::managed(&path.display().to_string(), "missing file name"))?
        .to_os_string();
    tmp_name.push(".agentos-tmp");
    let tmp = path.with_file_name(tmp_name);
    fs::write(&tmp, bytes).map_err(|err| AdapterError::io(&tmp, &err))?;
    fs::rename(&tmp, path).map_err(|err| {
        let _ = fs::remove_file(&tmp);
        AdapterError::io(path, &err)
    })
}

fn action(path: &str, kind: FileActionKind, detail: String) -> FileAction {
    FileAction {
        path: path.to_owned(),
        action: kind,
        detail,
    }
}

fn annotate(key: &str, err: AdapterError) -> AdapterError {
    match err {
        AdapterError::Io { message, .. } => AdapterError::Io {
            path: key.to_owned(),
            message,
        },
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    fn markers(body: &str) -> ManagedWrite {
        ManagedWrite {
            path: "CLAUDE.md".to_owned(),
            policy: WritePolicy::Markers {
                prelude: String::new(),
                body: body.to_owned(),
            },
        }
    }

    #[test]
    fn create_then_uninstall_deletes_the_file() {
        let dir = root();
        let report = install(dir.path(), "claude", &[markers("hello\n")]).unwrap();
        assert_eq!(report.files[0].action, FileActionKind::Created);
        assert!(dir.path().join("CLAUDE.md").is_file());
        let removed = uninstall(dir.path(), "claude").unwrap();
        assert_eq!(removed.files[0].action, FileActionKind::Removed);
        assert!(!dir.path().join("CLAUDE.md").exists());
    }

    #[test]
    fn preexisting_file_restores_bytes_when_nothing_else_changed() {
        let dir = root();
        let path = dir.path().join("CLAUDE.md");
        let original = b"# Title\n\nKeep me.\n";
        fs::write(&path, original).unwrap();
        install(dir.path(), "claude", &[markers("managed\n")]).unwrap();
        assert_ne!(fs::read(&path).unwrap(), original);
        uninstall(dir.path(), "claude").unwrap();
        assert_eq!(fs::read(&path).unwrap(), original);
    }

    #[test]
    fn user_edits_outside_the_block_survive_uninstall() {
        let dir = root();
        let path = dir.path().join("CLAUDE.md");
        fs::write(&path, b"# Title\n\nKeep me.\n").unwrap();
        install(dir.path(), "claude", &[markers("managed\n")]).unwrap();
        let edited = fs::read_to_string(&path)
            .unwrap()
            .replace("# Title", "# Title edited");
        fs::write(&path, &edited).unwrap();
        uninstall(dir.path(), "claude").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"# Title edited\n\nKeep me.\n");
    }

    #[test]
    fn user_text_after_the_block_is_kept() {
        let dir = root();
        let path = dir.path().join("NOTES.md");
        let original = b"before\n";
        fs::write(&path, original).unwrap();
        install(
            dir.path(),
            "codex",
            &[ManagedWrite {
                path: "NOTES.md".to_owned(),
                policy: WritePolicy::Markers {
                    prelude: String::new(),
                    body: "inside\n".to_owned(),
                },
            }],
        )
        .unwrap();
        let mut text = fs::read_to_string(&path).unwrap();
        text.push_str("after the block\n");
        fs::write(&path, &text).unwrap();
        uninstall(dir.path(), "codex").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"before\nafter the block\n");
    }

    #[test]
    fn reinstall_is_idempotent_and_update_touches_only_the_block() {
        let dir = root();
        let path = dir.path().join("CLAUDE.md");
        fs::write(&path, b"user\n").unwrap();
        install(dir.path(), "claude", &[markers("one\n")]).unwrap();
        let once = fs::read(&path).unwrap();
        let again = install(dir.path(), "claude", &[markers("one\n")]).unwrap();
        assert_eq!(again.files[0].action, FileActionKind::Unchanged);
        assert_eq!(fs::read(&path).unwrap(), once);
        install(dir.path(), "claude", &[markers("two\n")]).unwrap();
        let updated = fs::read_to_string(&path).unwrap();
        assert!(updated.starts_with("user\n"));
        assert!(updated.contains("two\n"));
        assert!(!updated.contains("one\n"));
    }

    #[test]
    fn empty_block_and_malformed_markers_are_safe() {
        let dir = root();
        let path = dir.path().join("CLAUDE.md");
        install(dir.path(), "claude", &[markers("")]).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains(BEGIN));
        assert!(text.contains(END));
        uninstall(dir.path(), "claude").unwrap();
        assert!(!path.exists());

        let broken = format!("{BEGIN}\nA\n{BEGIN}\nB\n{END}\n");
        fs::write(&path, &broken).unwrap();
        let err = install(dir.path(), "claude", &[markers("x\n")]).unwrap_err();
        assert!(
            err.to_string().contains("ambiguous")
                || err.to_string().contains("malformed")
                || err.to_string().contains("markers")
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), broken);
    }

    #[test]
    fn duplicate_markers_on_uninstall_do_not_delete_the_file() {
        let dir = root();
        install(dir.path(), "claude", &[markers("ok\n")]).unwrap();
        let path = dir.path().join("CLAUDE.md");
        let mut text = fs::read_to_string(&path).unwrap();
        text.push_str(BEGIN);
        text.push('\n');
        fs::write(&path, &text).unwrap();
        let report = uninstall(dir.path(), "claude").unwrap();
        assert_eq!(report.files[0].action, FileActionKind::Failed);
        assert!(path.is_file());
        assert!(fs::read_to_string(&path).unwrap().contains("ok\n"));
    }

    #[test]
    fn non_utf8_file_is_not_modified() {
        let dir = root();
        let path = dir.path().join("CLAUDE.md");
        fs::write(&path, [0xff, 0xfe, 0xfd]).unwrap();
        let err = install(dir.path(), "claude", &[markers("x\n")]).unwrap_err();
        assert!(err.to_string().contains("UTF-8"));
        assert_eq!(fs::read(&path).unwrap(), vec![0xff, 0xfe, 0xfd]);
    }

    #[cfg(unix)]
    #[test]
    fn read_only_directory_returns_an_error() {
        use std::os::unix::fs::PermissionsExt;
        let dir = root();
        let locked = dir.path().join("locked");
        fs::create_dir(&locked).unwrap();
        let mut perms = fs::metadata(&locked).unwrap().permissions();
        perms.set_mode(0o555);
        fs::set_permissions(&locked, perms).unwrap();
        let write = ManagedWrite {
            path: "locked/CLAUDE.md".to_owned(),
            policy: WritePolicy::Markers {
                prelude: String::new(),
                body: "x\n".to_owned(),
            },
        };
        let err = install(dir.path(), "claude", &[write]).unwrap_err();
        assert!(
            err.to_string().contains("locked/CLAUDE.md")
                || err.to_string().contains("Permission")
                || err.to_string().contains("denied")
                || err.to_string().contains("locked")
        );
        let mut perms = fs::metadata(&locked).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&locked, perms).unwrap();
        assert!(!locked.join("CLAUDE.md").exists());
    }

    #[test]
    fn whole_file_preexisting_is_kept_and_created_file_is_deleted() {
        let dir = root();
        let path = dir.path().join("skills.json");
        fs::write(&path, b"{}\n").unwrap();
        let write = ManagedWrite {
            path: "skills.json".to_owned(),
            policy: WritePolicy::Whole {
                contents: "{\"adapter\":\"generic\"}\n".to_owned(),
            },
        };
        let report = install(dir.path(), "generic", std::slice::from_ref(&write)).unwrap();
        assert_eq!(report.files[0].action, FileActionKind::Retained);
        assert_eq!(fs::read(&path).unwrap(), b"{}\n");

        fs::remove_file(&path).unwrap();
        let created = install(dir.path(), "generic", &[write]).unwrap();
        assert_eq!(created.files[0].action, FileActionKind::Created);
        let removed = uninstall(dir.path(), "generic").unwrap();
        assert_eq!(removed.files[0].action, FileActionKind::Removed);
        assert!(!path.exists());
    }

    #[test]
    fn whole_file_user_edit_is_retained_on_uninstall() {
        let dir = root();
        let path = dir.path().join("skills.json");
        let write = ManagedWrite {
            path: "skills.json".to_owned(),
            policy: WritePolicy::Whole {
                contents: "{\"adapter\":\"generic\"}\n".to_owned(),
            },
        };
        install(dir.path(), "generic", &[write]).unwrap();
        fs::write(&path, b"{\"adapter\":\"generic\",\"user\":true}\n").unwrap();
        let report = uninstall(dir.path(), "generic").unwrap();
        assert_eq!(report.files[0].action, FileActionKind::Retained);
        assert!(path.is_file());
    }

    #[test]
    fn created_directories_go_away_and_user_directories_stay() {
        let dir = root();
        fs::create_dir_all(dir.path().join(".cursor/rules")).unwrap();
        fs::write(dir.path().join(".cursor/rules/mine.mdc"), b"mine\n").unwrap();
        install(
            dir.path(),
            "cursor",
            &[ManagedWrite {
                path: ".cursor/rules/agentos.mdc".to_owned(),
                policy: WritePolicy::Markers {
                    prelude: "---\nalwaysApply: true\n---\n".to_owned(),
                    body: "rule\n".to_owned(),
                },
            }],
        )
        .unwrap();
        uninstall(dir.path(), "cursor").unwrap();
        assert!(dir.path().join(".cursor/rules/mine.mdc").is_file());
        assert!(!dir.path().join(".cursor/rules/agentos.mdc").exists());

        let other = root();
        install(
            other.path(),
            "cursor",
            &[ManagedWrite {
                path: ".cursor/rules/agentos.mdc".to_owned(),
                policy: WritePolicy::Markers {
                    prelude: String::new(),
                    body: "rule\n".to_owned(),
                },
            }],
        )
        .unwrap();
        uninstall(other.path(), "cursor").unwrap();
        assert!(!other.path().join(".cursor/rules").exists());
        assert!(!other.path().join(".cursor").exists());
    }

    #[test]
    fn pre_existing_gitignore_is_preserved_and_the_backup_restores() {
        let dir = root();
        let notes = dir.path().join("CLAUDE.md");
        let original = b"user notes\n";
        fs::write(&notes, original).unwrap();
        let ignore = dir.path().join(".agentos/.gitignore");
        fs::create_dir_all(ignore.parent().unwrap()).unwrap();
        let ignore_before = "keep-me\n# adapters/originals/\n";
        fs::write(&ignore, ignore_before).unwrap();

        install(dir.path(), "claude", &[markers("managed\n")]).unwrap();
        let ignore_after = fs::read_to_string(&ignore).unwrap();
        assert!(ignore_after.starts_with(ignore_before));
        assert!(ignore_after.ends_with("adapters/originals/\n"));
        assert_ne!(fs::read(&notes).unwrap(), original);
        let backups: Vec<_> = fs::read_dir(dir.path().join(".agentos/adapters/originals/claude"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        assert_eq!(backups.len(), 1);
        assert_eq!(fs::read(&backups[0]).unwrap(), original);

        let again = fs::read(&ignore).unwrap();
        install(dir.path(), "claude", &[markers("managed\n")]).unwrap();
        assert_eq!(fs::read(&ignore).unwrap(), again);

        uninstall(dir.path(), "claude").unwrap();
        assert_eq!(fs::read(&notes).unwrap(), original);
        assert_eq!(fs::read_to_string(&ignore).unwrap(), ignore_after);
        assert!(originals_ignored(dir.path()));
    }

    #[test]
    fn unsafe_paths_are_rejected() {
        let dir = root();
        let write = ManagedWrite {
            path: "../CLAUDE.md".to_owned(),
            policy: WritePolicy::Markers {
                prelude: String::new(),
                body: "x\n".to_owned(),
            },
        };
        assert!(install(dir.path(), "claude", &[write]).is_err());
    }
}
