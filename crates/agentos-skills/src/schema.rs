//! The published skill-manifest JSON Schema, embedded and compiled once.

use std::sync::LazyLock;

use serde_json::Value;

/// Contents of `schemas/skill-manifest.schema.json`.
pub const SKILL_MANIFEST_SCHEMA: &str = include_str!("../../../schemas/skill-manifest.schema.json");

static VALIDATOR: LazyLock<jsonschema::Validator> = LazyLock::new(|| {
    let schema: Value =
        serde_json::from_str(SKILL_MANIFEST_SCHEMA).expect("skill manifest schema is valid JSON");
    jsonschema::validator_for(&schema).expect("skill manifest schema compiles")
});

/// Validates `instance` against the manifest schema.
///
/// Returns `(instance_path, message)` pairs, sorted; empty means valid.
pub(crate) fn validate(instance: &Value) -> Vec<(String, String)> {
    let mut errors: Vec<(String, String)> = VALIDATOR
        .iter_errors(instance)
        .map(|e| {
            let path = e.instance_path().to_string();
            let path = if path.is_empty() {
                "/".to_owned()
            } else {
                path
            };
            (path, e.to_string())
        })
        .collect();
    errors.sort();
    errors.dedup();
    errors
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentos_permissions::Capability;
    use serde_json::json;

    #[test]
    fn schema_compiles() {
        assert!(validate(&json!({"name": "x", "description": "d", "version": "0.1.0"})).is_empty());
    }

    #[test]
    fn schema_permission_enum_matches_capabilities() {
        let schema: Value = serde_json::from_str(SKILL_MANIFEST_SCHEMA).unwrap();
        let listed: Vec<&str> = schema["properties"]["permissions"]["items"]["enum"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        let known: Vec<&str> = Capability::ALL.iter().map(|c| c.as_str()).collect();
        assert_eq!(listed, known);
    }

    #[test]
    fn schema_reports_paths() {
        let errors = validate(&json!({"name": "Bad Name", "version": 1, "extra": true}));
        let paths: Vec<&str> = errors.iter().map(|(p, _)| p.as_str()).collect();
        assert!(paths.contains(&"/name"), "{errors:?}");
        assert!(paths.contains(&"/version"), "{errors:?}");
        assert!(paths.contains(&"/"), "{errors:?}");
    }

    #[test]
    fn schema_requires_routes_only_for_routers() {
        let base = json!({"name": "r", "description": "d", "version": "0.1.0"});
        let mut router = base.clone();
        router["kind"] = json!("router");
        assert!(!validate(&router).is_empty(), "router without routes");
        router["routes"] = json!(["a"]);
        assert!(validate(&router).is_empty());
        let mut skill = base;
        skill["routes"] = json!(["a"]);
        assert!(!validate(&skill).is_empty(), "routes on a non-router");
    }
}
