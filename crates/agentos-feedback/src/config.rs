//! Feedback settings stored in `.agentos/config.toml`.
//!
//! `log` and `output` belong to `agentos-config`. This module reads only `[validation]` and
//! `[architecture]`. An explicit `providers` list replaces auto-detection.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::time::Duration;

use serde::Deserialize;

use crate::error::FeedbackError;

/// Default process timeout: 600 seconds.
pub const DEFAULT_TIMEOUT_SECS: u64 = 600;
/// Default stdout and stderr cap: 256 KiB each.
pub const DEFAULT_MAX_OUTPUT_BYTES: usize = 262_144;
/// A controller with more methods than this is fat.
pub const DEFAULT_FAT_CONTROLLER_METHODS: usize = 5;
/// A module with more classes and functions than this has an oversized public surface.
pub const DEFAULT_PUBLIC_SURFACE_MAX: usize = 6;

/// Effective validation settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationConfig {
    /// `Some` replaces auto-detection, including an empty list. `None` means detect.
    pub providers: Option<Vec<String>>,
    /// Process timeout.
    pub timeout: Duration,
    /// Byte cap applied to stdout and again to stderr.
    pub max_output_bytes: usize,
    /// Per-provider argv overrides, keyed by provider id.
    pub commands: BTreeMap<String, String>,
}

impl Default for ValidationConfig {
    fn default() -> Self {
        Self {
            providers: None,
            timeout: Duration::from_secs(DEFAULT_TIMEOUT_SECS),
            max_output_bytes: DEFAULT_MAX_OUTPUT_BYTES,
            commands: BTreeMap::new(),
        }
    }
}

/// Thresholds for architecture detectors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchitectureConfig {
    /// Method count greater than this flags a controller. Default [`DEFAULT_FAT_CONTROLLER_METHODS`].
    pub fat_controller_methods: usize,
    /// Class and function count greater than this flags a module. Default [`DEFAULT_PUBLIC_SURFACE_MAX`].
    pub public_surface_max: usize,
}

impl Default for ArchitectureConfig {
    fn default() -> Self {
        Self {
            fat_controller_methods: DEFAULT_FAT_CONTROLLER_METHODS,
            public_surface_max: DEFAULT_PUBLIC_SURFACE_MAX,
        }
    }
}

/// Both sections of the project file that this engine owns.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProjectConfig {
    /// Validation providers, timeout, output cap, and command overrides.
    pub validation: ValidationConfig,
    /// Architecture detector thresholds.
    pub architecture: ArchitectureConfig,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileConfig {
    #[serde(default)]
    log: Option<toml::Value>,
    #[serde(default)]
    output: Option<toml::Value>,
    #[serde(default)]
    validation: Option<ValidationSection>,
    #[serde(default)]
    architecture: Option<ArchitectureSection>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ValidationSection {
    #[serde(default)]
    providers: Option<Vec<String>>,
    #[serde(default)]
    timeout_secs: Option<u64>,
    #[serde(default)]
    max_output_bytes: Option<usize>,
    #[serde(default)]
    commands: BTreeMap<String, String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ArchitectureSection {
    #[serde(default)]
    fat_controller_methods: Option<usize>,
    #[serde(default)]
    public_surface_max: Option<usize>,
}

/// Reads `[validation]` and `[architecture]` from `path`. A missing file is the defaults.
pub fn load_project_config(path: &Path) -> Result<ProjectConfig, FeedbackError> {
    if !path.is_file() {
        return Ok(ProjectConfig::default());
    }
    let text = fs::read_to_string(path)?;
    let parsed: FileConfig = toml::from_str(&text)
        .map_err(|err| FeedbackError::Config(format!("{}: {}", path.display(), err.message())))?;
    let _ = (parsed.log, parsed.output);
    let mut config = ProjectConfig::default();
    if let Some(section) = parsed.validation {
        if let Some(secs) = section.timeout_secs {
            if secs == 0 {
                return Err(FeedbackError::Config(
                    "validation.timeout_secs must be greater than 0".to_owned(),
                ));
            }
            config.validation.timeout = Duration::from_secs(secs);
        }
        if let Some(cap) = section.max_output_bytes {
            if cap == 0 {
                return Err(FeedbackError::Config(
                    "validation.max_output_bytes must be greater than 0".to_owned(),
                ));
            }
            config.validation.max_output_bytes = cap;
        }
        config.validation.providers = section.providers;
        config.validation.commands = section.commands;
    }
    if let Some(section) = parsed.architecture {
        if let Some(value) = section.fat_controller_methods {
            config.architecture.fat_controller_methods = value;
        }
        if let Some(value) = section.public_surface_max {
            config.architecture.public_surface_max = value;
        }
    }
    Ok(config)
}
