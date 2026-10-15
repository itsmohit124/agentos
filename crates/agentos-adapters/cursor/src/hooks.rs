//! Cursor `beforeSubmitPrompt` hook files.

use std::path::Path;

use serde_json::{Value, json};

use agentos_adapter_generic::{AdapterError, ManagedWrite, WritePolicy};

const HOOK_SCRIPT: &str = include_str!("../hooks/agentos-context.py");
const HOOK_REL: &str = ".cursor/hooks/agentos-context.py";
const HOOKS_JSON_REL: &str = ".cursor/hooks.json";
const HOOK_COMMAND: &str = "python3 .cursor/hooks/agentos-context.py";

/// Hook script plus a `hooks.json` that registers `beforeSubmitPrompt`.
pub fn artifacts(project: &Path) -> Result<Vec<ManagedWrite>, AdapterError> {
    Ok(vec![
        ManagedWrite {
            path: HOOK_REL.to_owned(),
            policy: WritePolicy::Whole {
                contents: ensure_trailing_newline(HOOK_SCRIPT),
            },
        },
        ManagedWrite {
            path: HOOKS_JSON_REL.to_owned(),
            policy: WritePolicy::Whole {
                contents: merge_hooks_json(project)?,
            },
        },
    ])
}

fn ensure_trailing_newline(text: &str) -> String {
    if text.ends_with('\n') {
        text.to_owned()
    } else {
        format!("{text}\n")
    }
}

fn merge_hooks_json(project: &Path) -> Result<String, AdapterError> {
    let path = project.join(HOOKS_JSON_REL);
    let mut root = if path.is_file() {
        let raw = std::fs::read_to_string(&path).map_err(|err| AdapterError::Managed {
            path: HOOKS_JSON_REL.to_owned(),
            message: format!("cannot read hooks.json: {err}"),
        })?;
        serde_json::from_str::<Value>(&raw).unwrap_or_else(|_| json!({}))
    } else {
        json!({})
    };
    if !root.is_object() {
        root = json!({});
    }
    let object = root.as_object_mut().expect("object");
    object
        .entry("version")
        .or_insert(Value::from(1));
    let hooks = object
        .entry("hooks")
        .or_insert_with(|| json!({}));
    if !hooks.is_object() {
        *hooks = json!({});
    }
    let hooks_obj = hooks.as_object_mut().expect("hooks object");
    let list = hooks_obj
        .entry("beforeSubmitPrompt")
        .or_insert_with(|| json!([]));
    if !list.is_array() {
        *list = json!([]);
    }
    let entries = list.as_array_mut().expect("array");
    let already = entries.iter().any(|entry| {
        entry
            .get("command")
            .and_then(Value::as_str)
            .is_some_and(|command| command == HOOK_COMMAND)
    });
    if !already {
        entries.push(json!({
            "command": HOOK_COMMAND,
            "timeout": 30
        }));
    }
    let mut encoded = serde_json::to_string_pretty(&root).map_err(|err| AdapterError::Managed {
        path: HOOKS_JSON_REL.to_owned(),
        message: format!("cannot write hooks.json: {err}"),
    })?;
    if !encoded.ends_with('\n') {
        encoded.push('\n');
    }
    Ok(encoded)
}
