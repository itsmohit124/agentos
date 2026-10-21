//! Native invocation controls for skills Cursor and Codex discover.
//!
//! AgentOS `invocation` stays the contract. Cursor reads `disable-model-invocation` in
//! `SKILL.md`. Codex reads `agents/openai.yaml`. Install writes those controls onto
//! user-only skills under `.agents/skills/` and does not change any other line. A
//! model-invocable skill is left untouched. Uninstall does not remove the controls:
//! removing them would make a user-only skill model-invocable again.

use std::fs;
use std::path::{Path, PathBuf};

use agentos_skills::{declares_version, parse_manifest};

use crate::error::AdapterError;

const DISABLE_LINE: &str = "disable-model-invocation: true";
const CODEX_POLICY: &str = "policy:\n  allow_implicit_invocation: false\n";
const CODEX_LINE: &str = "allow_implicit_invocation: false";

/// Writes the native controls. A second call does not change a file that is already sealed.
/// A skill with no `version` is a host skill and is left unchanged.
pub fn seal(root: &Path) -> Result<(), AdapterError> {
    for skill in scan(root)? {
        let Some(skill) =
            skill.map_err(|message| AdapterError::managed(".agents/skills", message))?
        else {
            continue;
        };
        if skill.model_invocable {
            continue;
        }
        let write_policy = codex_needs_write(&skill)?;
        seal_frontmatter(root, &skill)?;
        if write_policy {
            write_codex(root, &skill)?;
        }
    }
    Ok(())
}

/// User-only project skills whose native controls are missing or contradictory.
pub fn problems(root: &Path) -> Result<Vec<String>, AdapterError> {
    let mut gaps = Vec::new();
    for skill in scan(root)? {
        let skill = match skill {
            Ok(Some(skill)) => skill,
            Ok(None) => continue,
            Err(message) => {
                gaps.push(message);
                continue;
            }
        };
        if skill.model_invocable {
            continue;
        }
        let text =
            fs::read_to_string(&skill.file).map_err(|err| AdapterError::io(&skill.file, &err))?;
        if !frontmatter_disables_model(&text) {
            gaps.push(format!(
                "{} is missing {DISABLE_LINE}",
                skill.file.display()
            ));
        }
        let policy = skill.dir.join("agents/openai.yaml");
        let yaml = fs::read_to_string(&policy).unwrap_or_default();
        if !yaml.lines().any(|line| line.trim() == CODEX_LINE) {
            gaps.push(format!("{} is missing {CODEX_LINE}", policy.display()));
        }
    }
    gaps.sort();
    Ok(gaps)
}

struct ProjectSkill {
    file: PathBuf,
    dir: PathBuf,
    model_invocable: bool,
}

fn scan(root: &Path) -> Result<Vec<Result<Option<ProjectSkill>, String>>, AdapterError> {
    let base = root.join(".agents/skills");
    if !base.is_dir() {
        return Ok(Vec::new());
    }
    let mut files = Vec::new();
    collect(&base, &mut files)?;
    files.sort();
    let mut skills = Vec::new();
    for file in files {
        skills.push(read_skill(&file));
    }
    Ok(skills)
}

fn collect(dir: &Path, files: &mut Vec<PathBuf>) -> Result<(), AdapterError> {
    let entries = fs::read_dir(dir).map_err(|err| AdapterError::io(dir, &err))?;
    for entry in entries {
        let entry = entry.map_err(|err| AdapterError::io(dir, &err))?;
        let path = entry.path();
        let meta = fs::symlink_metadata(&path).map_err(|err| AdapterError::io(&path, &err))?;
        if meta.file_type().is_symlink() {
            continue;
        }
        if meta.is_dir() {
            collect(&path, files)?;
        } else if path.file_name().and_then(|name| name.to_str()) == Some("SKILL.md") {
            files.push(path);
        }
    }
    Ok(())
}

fn read_skill(file: &Path) -> Result<Option<ProjectSkill>, String> {
    let text = fs::read_to_string(file).map_err(|err| format!("{}: {err}", file.display()))?;
    let yaml = frontmatter_yaml(&text).map_err(|err| format!("{}: {err}", file.display()))?;
    let owned = declares_version(&yaml).map_err(|err| format!("{}: {err}", file.display()))?;
    if !owned {
        return Ok(None);
    }
    let name = file
        .parent()
        .and_then(|dir| dir.file_name())
        .and_then(|name| name.to_str())
        .unwrap_or("skill");
    let manifest = parse_manifest(&yaml, Some(name), &file.display().to_string())
        .map_err(|err| format!("{}: {}", file.display(), err[0].message))?;
    Ok(Some(ProjectSkill {
        file: file.to_path_buf(),
        dir: file.parent().unwrap_or(file).to_path_buf(),
        model_invocable: manifest.invocation.model_invocable,
    }))
}

fn seal_frontmatter(root: &Path, skill: &ProjectSkill) -> Result<(), AdapterError> {
    let text =
        fs::read_to_string(&skill.file).map_err(|err| AdapterError::io(&skill.file, &err))?;
    if frontmatter_disables_model(&text) {
        return Ok(());
    }
    if frontmatter_sets_disable(&text).is_some() {
        return Err(AdapterError::managed(
            &skill.file.display().to_string(),
            "disable-model-invocation contradicts a user-only skill; file was not modified",
        ));
    }
    let updated = insert_disable_line(&text)
        .map_err(|err| AdapterError::managed(&skill.file.display().to_string(), err))?;
    remember(root, &skill.file, text.as_bytes())?;
    fs::write(&skill.file, updated).map_err(|err| AdapterError::io(&skill.file, &err))?;
    Ok(())
}

fn codex_needs_write(skill: &ProjectSkill) -> Result<bool, AdapterError> {
    let path = skill.dir.join("agents/openai.yaml");
    if path.is_file() {
        let text = fs::read_to_string(&path).map_err(|err| AdapterError::io(&path, &err))?;
        if text.lines().any(|line| line.trim() == CODEX_LINE) {
            return Ok(false);
        }
        return Err(AdapterError::managed(
            &path.display().to_string(),
            "existing Codex policy does not disable implicit invocation; file was not modified",
        ));
    }
    if path.exists() {
        return Err(AdapterError::managed(
            &path.display().to_string(),
            "Codex policy path is not a file",
        ));
    }
    Ok(true)
}

fn write_codex(root: &Path, skill: &ProjectSkill) -> Result<(), AdapterError> {
    let path = skill.dir.join("agents/openai.yaml");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| AdapterError::io(parent, &err))?;
    }
    remember(root, &path, b"")?;
    fs::write(&path, CODEX_POLICY).map_err(|err| AdapterError::io(&path, &err))?;
    Ok(())
}

fn remember(root: &Path, source: &Path, bytes: &[u8]) -> Result<(), AdapterError> {
    let relative = source.strip_prefix(root).unwrap_or(source);
    let dest = root
        .join(".agentos/adapters/originals/invocation")
        .join(relative);
    if dest.exists() {
        return Ok(());
    }
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|err| AdapterError::io(parent, &err))?;
    }
    fs::write(&dest, bytes).map_err(|err| AdapterError::io(&dest, &err))
}

fn frontmatter_disables_model(text: &str) -> bool {
    frontmatter_sets_disable(text).is_some_and(|value| value == "true")
}

fn frontmatter_sets_disable(text: &str) -> Option<String> {
    let yaml = frontmatter_yaml(text).ok()?;
    for line in yaml.lines() {
        let trimmed = line.trim();
        let Some(value) = trimmed.strip_prefix("disable-model-invocation:") else {
            continue;
        };
        return Some(value.trim().to_owned());
    }
    None
}

fn frontmatter_yaml(text: &str) -> Result<String, String> {
    let (body, _) = split_frontmatter(text)?;
    Ok(body)
}

fn insert_disable_line(text: &str) -> Result<String, String> {
    let (_, close_at) = split_frontmatter(text)?;
    let mut updated = String::new();
    updated.push_str(&text[..close_at]);
    if !updated.ends_with('\n') {
        updated.push('\n');
    }
    updated.push_str(DISABLE_LINE);
    updated.push('\n');
    updated.push_str(&text[close_at..]);
    Ok(updated)
}

/// YAML inside the first front matter, and the byte index of its closing marker.
fn split_frontmatter(text: &str) -> Result<(String, usize), String> {
    let newline = if text.starts_with("---\r\n") {
        5
    } else if text.starts_with("---\n") {
        4
    } else {
        return Err("SKILL.md has no front matter; file was not modified".to_owned());
    };
    let rest = &text[newline..];
    let mut offset = newline;
    for line in rest.split_inclusive('\n') {
        let bare = line.trim_end_matches(['\r', '\n']);
        if bare == "---" {
            return Ok((text[newline..offset].to_owned(), offset));
        }
        offset += line.len();
    }
    Err("front matter is not closed; file was not modified".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::managed;
    use crate::types::{ManagedWrite, WritePolicy};

    fn skill(dir: &Path, name: &str, invocation: &str) {
        let folder = dir.join(".agents/skills").join(name);
        fs::create_dir_all(&folder).unwrap();
        fs::write(
            folder.join("SKILL.md"),
            format!(
                "---\nname: {name}\ndescription: Native seal.\nversion: \"0.1.0\"\ninvocation: {invocation}\n---\n\n# Body\n"
            ),
        )
        .unwrap();
    }

    #[test]
    fn user_only_skill_gains_native_controls_and_keeps_them_after_uninstall() {
        let dir = tempfile::tempdir().unwrap();
        skill(dir.path(), "manual-only", "user");
        let file = dir.path().join(".agents/skills/manual-only/SKILL.md");
        let before = fs::read(&file).unwrap();
        seal(dir.path()).unwrap();
        let after = fs::read_to_string(&file).unwrap();
        assert!(after.contains("disable-model-invocation: true\n"));
        assert!(after.contains("invocation: user\n"));
        assert!(after.contains("# Body\n"));
        assert_eq!(after.matches("disable-model-invocation").count(), 1);
        let policy = fs::read_to_string(
            dir.path()
                .join(".agents/skills/manual-only/agents/openai.yaml"),
        )
        .unwrap();
        assert!(policy.contains("allow_implicit_invocation: false"));
        assert!(problems(dir.path()).unwrap().is_empty());
        let again = fs::read(&file).unwrap();
        seal(dir.path()).unwrap();
        assert_eq!(fs::read(&file).unwrap(), again);

        managed::install(
            dir.path(),
            "generic",
            &[ManagedWrite {
                path: "NOTES.md".to_owned(),
                policy: WritePolicy::Markers {
                    prelude: String::new(),
                    body: "note\n".to_owned(),
                },
            }],
        )
        .unwrap();
        managed::uninstall(dir.path(), "generic").unwrap();
        assert!(!dir.path().join("NOTES.md").exists());
        assert_eq!(fs::read_to_string(&file).unwrap(), after);
        assert!(before != after.as_bytes());
        let saved = dir
            .path()
            .join(".agentos/adapters/originals/invocation/.agents/skills/manual-only/SKILL.md");
        assert_eq!(fs::read(saved).unwrap(), before);
    }

    #[test]
    fn a_host_skill_without_version_does_not_block_install() {
        let dir = tempfile::tempdir().unwrap();
        skill(dir.path(), "manual-only", "user");
        let host = dir.path().join(".agents/skills/codebase-design");
        fs::create_dir_all(&host).unwrap();
        let host_file = host.join("SKILL.md");
        let host_text =
            "---\nname: codebase-design\ndescription: Shared vocabulary.\n---\n\n# Body\n";
        fs::write(&host_file, host_text).unwrap();

        seal(dir.path()).unwrap();

        assert_eq!(fs::read_to_string(&host_file).unwrap(), host_text);
        let sealed =
            fs::read_to_string(dir.path().join(".agents/skills/manual-only/SKILL.md")).unwrap();
        assert!(sealed.contains("disable-model-invocation: true\n"));
        assert!(problems(dir.path()).unwrap().is_empty());
        managed::install(dir.path(), "generic", &[]).unwrap();
    }

    #[test]
    fn model_invocable_skill_is_not_sealed() {
        let dir = tempfile::tempdir().unwrap();
        skill(dir.path(), "either", "both");
        let file = dir.path().join(".agents/skills/either/SKILL.md");
        let before = fs::read(&file).unwrap();
        seal(dir.path()).unwrap();
        assert_eq!(fs::read(&file).unwrap(), before);
        assert!(
            !dir.path()
                .join(".agents/skills/either/agents/openai.yaml")
                .exists()
        );
    }

    #[test]
    fn a_contradicting_host_flag_is_left_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join(".agents/skills/manual-only");
        fs::create_dir_all(&folder).unwrap();
        let text = "---\nname: manual-only\ndescription: Native seal.\nversion: \"0.1.0\"\ninvocation: user\ndisable-model-invocation: false\n---\n";
        let file = folder.join("SKILL.md");
        fs::write(&file, text).unwrap();
        let err = seal(dir.path()).unwrap_err();
        assert!(
            err.to_string().contains("contradicts")
                || err.to_string().contains("not modified")
                || err.to_string().contains("disable-model-invocation")
        );
        assert_eq!(fs::read_to_string(&file).unwrap(), text);
    }

    #[test]
    fn an_existing_codex_policy_is_not_rewritten() {
        let dir = tempfile::tempdir().unwrap();
        skill(dir.path(), "manual-only", "user");
        let file = dir.path().join(".agents/skills/manual-only/SKILL.md");
        let before = fs::read(&file).unwrap();
        let policy_dir = dir.path().join(".agents/skills/manual-only/agents");
        fs::create_dir_all(&policy_dir).unwrap();
        let policy = policy_dir.join("openai.yaml");
        fs::write(&policy, "policy:\n  allow_implicit_invocation: true\n").unwrap();
        let err = seal(dir.path()).unwrap_err();
        assert!(err.to_string().contains("was not modified"));
        assert_eq!(fs::read(&file).unwrap(), before);
        assert_eq!(
            fs::read_to_string(&policy).unwrap(),
            "policy:\n  allow_implicit_invocation: true\n"
        );
    }
}
