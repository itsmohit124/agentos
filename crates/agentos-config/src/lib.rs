//! AgentOS layered configuration.
//!
//! Effective configuration is resolved from four layers, later layers winning:
//!
//! 1. built-in defaults
//! 2. the project file `.agentos/config.toml`
//! 3. environment variables (`AGENTOS_*`, see [`ENV_VARS`])
//! 4. CLI flags ([`ConfigOverrides`])
//!
//! Every resolved value records which layer it came from ([`ConfigSource`]).

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use agentos_core::fs::{FileSystem, FsError};
use serde::Deserialize;

/// Config key for the log filter.
pub const KEY_LOG_LEVEL: &str = "log.level";
/// Config key for the default output format.
pub const KEY_OUTPUT_FORMAT: &str = "output.format";

/// Environment variables recognised by the environment layer, and the key each one sets.
pub const ENV_VARS: &[(&str, &str)] = &[
    ("AGENTOS_LOG", KEY_LOG_LEVEL),
    ("AGENTOS_OUTPUT", KEY_OUTPUT_FORMAT),
];

const DEFAULT_LOG_LEVEL: &str = "warn";
const LEVELS: &[&str] = &["off", "error", "warn", "info", "debug", "trace"];

/// Commented default `config.toml` written by `agentos init`.
pub const DEFAULT_CONFIG_TOML: &str = r#"# AgentOS project configuration.
# Precedence (later wins): built-in defaults < this file < AGENTOS_* environment variables < CLI flags.

[log]
# Log filter: a level (off|error|warn|info|debug|trace) or tracing directives,
# e.g. "agentos_core=debug,warn". Env: AGENTOS_LOG. Flags: --log-level, -v, -vv.
level = "warn"

[output]
# Default output format: "text" or "json". Env: AGENTOS_OUTPUT. Flag: --json.
format = "text"

# Optional validation overrides. Uncomment to replace auto-detection.
# [validation]
# providers = ["pytest"]
# timeout_secs = 600
# max_output_bytes = 262144
#
# [validation.commands]
# pytest = "python3 -m pytest -q --tb=short --color=no"
#
# [architecture]
# A controller is fat when its method count is greater than this number.
# fat_controller_methods = 5
# A module is oversized when it has more public classes and functions than this.
# public_surface_max = 6
"#;

/// Output format for command results.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutputFormat {
    /// Human-readable text.
    #[default]
    Text,
    /// Machine-readable JSON.
    Json,
}

impl OutputFormat {
    /// Stable name as used in config files.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Json => "json",
        }
    }
}

impl fmt::Display for OutputFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for OutputFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "text" => Ok(Self::Text),
            "json" => Ok(Self::Json),
            _ => Err("expected \"text\" or \"json\"".to_owned()),
        }
    }
}

/// Effective AgentOS configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// Log filter (a level or tracing directives). Key `log.level`.
    pub log_level: String,
    /// Default output format. Key `output.format`.
    pub output_format: OutputFormat,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            log_level: DEFAULT_LOG_LEVEL.to_owned(),
            output_format: OutputFormat::Text,
        }
    }
}

impl Config {
    /// All keys with their values rendered as strings, in a fixed order.
    pub fn entries(&self) -> Vec<(&'static str, String)> {
        vec![
            (KEY_LOG_LEVEL, self.log_level.clone()),
            (KEY_OUTPUT_FORMAT, self.output_format.as_str().to_owned()),
        ]
    }
}

/// Values given on the command line. `None` means "not given".
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConfigOverrides {
    /// Overrides `log.level`.
    pub log_level: Option<String>,
    /// Overrides `output.format`.
    pub output_format: Option<OutputFormat>,
}

/// Layer a resolved value came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigSource {
    /// Built-in default.
    Default,
    /// The project config file.
    File,
    /// An environment variable.
    Env(&'static str),
    /// A command-line flag.
    Cli,
}

impl fmt::Display for ConfigSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Default => f.write_str("default"),
            Self::File => f.write_str("file"),
            Self::Env(var) => write!(f, "env:{var}"),
            Self::Cli => f.write_str("cli"),
        }
    }
}

/// Configuration together with where each value came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedConfig {
    /// The effective configuration.
    pub config: Config,
    /// Source of every key, keyed by config key.
    pub sources: BTreeMap<&'static str, ConfigSource>,
    /// Path of the config file that was considered.
    pub file: PathBuf,
    /// Whether that file existed and was loaded.
    pub file_loaded: bool,
}

impl ResolvedConfig {
    /// Source of `key`.
    pub fn source(&self, key: &str) -> Option<&ConfigSource> {
        self.sources.get(key)
    }
}

/// Error returned by [`resolve`].
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// The config file could not be read.
    #[error("cannot read config: {0}")]
    Read(#[from] FsError),
    /// The config file is not valid TOML or contains unknown keys.
    #[error("invalid config file {}: {message}", path.display())]
    Parse {
        /// Path of the config file.
        path: PathBuf,
        /// Parser message.
        message: String,
    },
    /// A value is invalid.
    #[error("invalid value {value:?} for {key} (from {origin}): {reason}")]
    InvalidValue {
        /// Config key.
        key: &'static str,
        /// Offending value.
        value: String,
        /// Layer the value came from.
        origin: ConfigSource,
        /// Why it is invalid.
        reason: String,
    },
}

/// Snapshot of the `AGENTOS_*` environment variables.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EnvVars(BTreeMap<String, String>);

impl EnvVars {
    /// Captures the `AGENTOS_*` variables of the current process.
    pub fn from_process() -> Self {
        Self::from_pairs(std::env::vars())
    }

    /// Builds from arbitrary pairs, keeping only `AGENTOS_*` variables.
    pub fn from_pairs<K: Into<String>, V: Into<String>>(
        pairs: impl IntoIterator<Item = (K, V)>,
    ) -> Self {
        Self(
            pairs
                .into_iter()
                .map(|(k, v)| (k.into(), v.into()))
                .filter(|(k, _)| k.starts_with("AGENTOS_"))
                .collect(),
        )
    }

    fn get(&self, name: &str) -> Option<&str> {
        self.0.get(name).map(String::as_str)
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileConfig {
    log: Option<FileLog>,
    output: Option<FileOutput>,
    /// Accepted so a project can store provider overrides. Resolved by `agentos-feedback`.
    #[serde(default)]
    #[allow(dead_code)]
    validation: Option<toml::Value>,
    /// Accepted so a project can store architecture thresholds. Resolved by `agentos-feedback`.
    #[serde(default)]
    #[allow(dead_code)]
    architecture: Option<toml::Value>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileLog {
    level: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileOutput {
    format: Option<String>,
}

/// A raw value from one layer, before validation.
struct Raw {
    value: String,
    source: ConfigSource,
}

/// Resolves the effective configuration.
///
/// A missing `config_file` is not an error (the file layer is simply empty).
pub fn resolve(
    fs: &dyn FileSystem,
    config_file: &Path,
    env: &EnvVars,
    overrides: &ConfigOverrides,
) -> Result<ResolvedConfig, ConfigError> {
    let (file, file_loaded) = load_file(fs, config_file)?;
    let mut layers: BTreeMap<&'static str, Raw> = BTreeMap::new();

    let defaults = Config::default();
    for (key, value) in defaults.entries() {
        layers.insert(
            key,
            Raw {
                value,
                source: ConfigSource::Default,
            },
        );
    }

    let file_values = [
        (KEY_LOG_LEVEL, file.log.and_then(|l| l.level)),
        (KEY_OUTPUT_FORMAT, file.output.and_then(|o| o.format)),
    ];
    for (key, value) in file_values {
        if let Some(value) = value {
            layers.insert(
                key,
                Raw {
                    value,
                    source: ConfigSource::File,
                },
            );
        }
    }

    for &(var, key) in ENV_VARS {
        if let Some(value) = env.get(var) {
            layers.insert(
                key,
                Raw {
                    value: value.to_owned(),
                    source: ConfigSource::Env(var),
                },
            );
        }
    }

    let cli_values = [
        (KEY_LOG_LEVEL, overrides.log_level.clone()),
        (
            KEY_OUTPUT_FORMAT,
            overrides.output_format.map(|f| f.as_str().to_owned()),
        ),
    ];
    for (key, value) in cli_values {
        if let Some(value) = value {
            layers.insert(
                key,
                Raw {
                    value,
                    source: ConfigSource::Cli,
                },
            );
        }
    }

    let log = &layers[KEY_LOG_LEVEL];
    validate_log_filter(&log.value).map_err(|reason| invalid(KEY_LOG_LEVEL, log, reason))?;
    let out = &layers[KEY_OUTPUT_FORMAT];
    let output_format = out
        .value
        .parse::<OutputFormat>()
        .map_err(|reason| invalid(KEY_OUTPUT_FORMAT, out, reason))?;

    let config = Config {
        log_level: log.value.clone(),
        output_format,
    };
    let sources = layers.into_iter().map(|(k, raw)| (k, raw.source)).collect();
    Ok(ResolvedConfig {
        config,
        sources,
        file: config_file.to_path_buf(),
        file_loaded,
    })
}

fn invalid(key: &'static str, raw: &Raw, reason: String) -> ConfigError {
    ConfigError::InvalidValue {
        key,
        value: raw.value.clone(),
        origin: raw.source.clone(),
        reason,
    }
}

fn load_file(fs: &dyn FileSystem, path: &Path) -> Result<(FileConfig, bool), ConfigError> {
    if !fs.exists(path) {
        return Ok((FileConfig::default(), false));
    }
    let text = fs.read_to_string(path)?;
    let parsed = toml::from_str::<FileConfig>(&text).map_err(|e| ConfigError::Parse {
        path: path.to_path_buf(),
        message: e.message().to_owned(),
    })?;
    Ok((parsed, true))
}

/// Validates a log filter: comma-separated `level` or `target=level` directives.
fn validate_log_filter(filter: &str) -> Result<(), String> {
    let directives: Vec<&str> = filter.split(',').map(str::trim).collect();
    if directives.iter().all(|d| d.is_empty()) {
        return Err("must not be empty".to_owned());
    }
    for directive in directives.into_iter().filter(|d| !d.is_empty()) {
        let level = match directive.split_once('=') {
            Some((target, level)) => {
                let valid_target = !target.is_empty()
                    && target
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | ':' | '-'));
                if !valid_target {
                    return Err(format!("invalid target in directive {directive:?}"));
                }
                level
            }
            None => directive,
        };
        if !LEVELS.contains(&level.to_ascii_lowercase().as_str()) {
            return Err(format!(
                "unknown level {level:?} (expected one of {})",
                LEVELS.join("|")
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentos_core::fs::MemFs;

    const CFG: &str = "/p/.agentos/config.toml";

    fn fs_with_file(contents: Option<&str>) -> MemFs {
        let fs = MemFs::new();
        fs.create_dir_all(Path::new("/p/.agentos")).unwrap();
        if let Some(c) = contents {
            fs.write(Path::new(CFG), c.as_bytes()).unwrap();
        }
        fs
    }

    fn run(
        file: Option<&str>,
        env: &[(&str, &str)],
        overrides: ConfigOverrides,
    ) -> Result<ResolvedConfig, ConfigError> {
        let fs = fs_with_file(file);
        resolve(
            &fs,
            Path::new(CFG),
            &EnvVars::from_pairs(env.iter().copied()),
            &overrides,
        )
    }

    const FILE: &str = "[log]\nlevel = \"info\"\n[output]\nformat = \"json\"\n";

    #[test]
    fn defaults_only() {
        let r = run(None, &[], ConfigOverrides::default()).unwrap();
        assert_eq!(r.config, Config::default());
        assert!(!r.file_loaded);
        assert_eq!(r.source(KEY_LOG_LEVEL), Some(&ConfigSource::Default));
        assert_eq!(r.source(KEY_OUTPUT_FORMAT), Some(&ConfigSource::Default));
    }

    #[test]
    fn file_overrides_defaults() {
        let r = run(Some(FILE), &[], ConfigOverrides::default()).unwrap();
        assert!(r.file_loaded);
        assert_eq!(r.config.log_level, "info");
        assert_eq!(r.config.output_format, OutputFormat::Json);
        assert_eq!(r.source(KEY_LOG_LEVEL), Some(&ConfigSource::File));
    }

    #[test]
    fn partial_file_keeps_other_defaults() {
        let r = run(
            Some("[log]\nlevel = \"debug\"\n"),
            &[],
            ConfigOverrides::default(),
        )
        .unwrap();
        assert_eq!(r.config.log_level, "debug");
        assert_eq!(r.config.output_format, OutputFormat::Text);
        assert_eq!(r.source(KEY_OUTPUT_FORMAT), Some(&ConfigSource::Default));
    }

    #[test]
    fn env_overrides_file() {
        let r = run(
            Some(FILE),
            &[("AGENTOS_LOG", "trace"), ("AGENTOS_OUTPUT", "text")],
            ConfigOverrides::default(),
        )
        .unwrap();
        assert_eq!(r.config.log_level, "trace");
        assert_eq!(r.config.output_format, OutputFormat::Text);
        assert_eq!(
            r.source(KEY_LOG_LEVEL),
            Some(&ConfigSource::Env("AGENTOS_LOG"))
        );
        assert_eq!(
            r.source(KEY_OUTPUT_FORMAT),
            Some(&ConfigSource::Env("AGENTOS_OUTPUT"))
        );
    }

    #[test]
    fn cli_overrides_env_and_file() {
        let r = run(
            Some(FILE),
            &[("AGENTOS_LOG", "trace"), ("AGENTOS_OUTPUT", "text")],
            ConfigOverrides {
                log_level: Some("error".into()),
                output_format: Some(OutputFormat::Json),
            },
        )
        .unwrap();
        assert_eq!(r.config.log_level, "error");
        assert_eq!(r.config.output_format, OutputFormat::Json);
        assert_eq!(r.source(KEY_LOG_LEVEL), Some(&ConfigSource::Cli));
        assert_eq!(r.source(KEY_OUTPUT_FORMAT), Some(&ConfigSource::Cli));
    }

    #[test]
    fn layers_resolve_independently_per_key() {
        let r = run(
            Some("[output]\nformat = \"json\"\n"),
            &[("AGENTOS_LOG", "info")],
            ConfigOverrides::default(),
        )
        .unwrap();
        assert_eq!(
            r.source(KEY_LOG_LEVEL),
            Some(&ConfigSource::Env("AGENTOS_LOG"))
        );
        assert_eq!(r.source(KEY_OUTPUT_FORMAT), Some(&ConfigSource::File));
    }

    #[test]
    fn unrelated_env_vars_are_ignored() {
        let r = run(
            None,
            &[("AGENTOS_UNKNOWN", "x"), ("LOG", "trace")],
            ConfigOverrides::default(),
        )
        .unwrap();
        assert_eq!(r.config, Config::default());
    }

    #[test]
    fn default_config_toml_resolves_to_defaults() {
        let r = run(Some(DEFAULT_CONFIG_TOML), &[], ConfigOverrides::default()).unwrap();
        assert!(r.file_loaded);
        assert_eq!(r.config, Config::default());
    }

    #[test]
    fn validation_and_architecture_tables_do_not_break_resolution() {
        let text = r#"
[log]
level = "info"

[validation]
providers = ["pytest"]
timeout_secs = 30

[validation.commands]
pytest = "python3 -m pytest -q"

[architecture]
fat_controller_methods = 5
public_surface_max = 6
"#;
        let r = run(Some(text), &[], ConfigOverrides::default()).unwrap();
        assert_eq!(r.config.log_level, "info");
        assert_eq!(r.source(KEY_LOG_LEVEL), Some(&ConfigSource::File));
    }

    #[test]
    fn log_directives_are_accepted() {
        let r = run(
            None,
            &[("AGENTOS_LOG", "agentos_core=debug, warn")],
            ConfigOverrides::default(),
        )
        .unwrap();
        assert_eq!(r.config.log_level, "agentos_core=debug, warn");
    }

    #[test]
    fn invalid_log_level_reports_key_and_source() {
        let err = run(None, &[("AGENTOS_LOG", "loud")], ConfigOverrides::default()).unwrap_err();
        match err {
            ConfigError::InvalidValue { key, origin, .. } => {
                assert_eq!(key, KEY_LOG_LEVEL);
                assert_eq!(origin, ConfigSource::Env("AGENTOS_LOG"));
            }
            other => panic!("unexpected error: {other}"),
        }
    }

    #[test]
    fn invalid_values_are_rejected() {
        for bad in ["", " , ", "a b=info", "=info"] {
            assert!(
                validate_log_filter(bad).is_err(),
                "{bad:?} should be invalid"
            );
        }
        let err = run(
            Some("[output]\nformat = \"xml\"\n"),
            &[],
            ConfigOverrides::default(),
        )
        .unwrap_err();
        assert!(matches!(
            err,
            ConfigError::InvalidValue {
                key: KEY_OUTPUT_FORMAT,
                origin: ConfigSource::File,
                ..
            }
        ));
    }

    #[test]
    fn unknown_keys_and_bad_toml_are_parse_errors() {
        for bad in [
            "[log]\nlevle = \"info\"\n",
            "[future]\nx = 1\n",
            "not toml [",
        ] {
            assert!(
                matches!(
                    run(Some(bad), &[], ConfigOverrides::default()),
                    Err(ConfigError::Parse { .. })
                ),
                "{bad:?} should fail to parse"
            );
        }
    }
}
