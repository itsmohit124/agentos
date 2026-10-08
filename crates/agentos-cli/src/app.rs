//! Per-invocation application context.

use std::path::Path;

use agentos_config::{Config, ConfigError, ConfigOverrides, EnvVars, OutputFormat, ResolvedConfig};
use agentos_core::fs::RealFs;
use agentos_core::git::GitCli;
use agentos_core::root::{ProjectRoot, detect_root};
use agentos_core::state::StateLayout;
use anyhow::Context;

/// Everything a command needs: real I/O implementations, project root and configuration.
pub struct App {
    pub fs: RealFs,
    pub git: GitCli,
    pub root: ProjectRoot,
    pub layout: StateLayout,
    /// Configuration resolution result. Kept as a `Result` so `doctor` can report invalid config.
    pub config: Result<ResolvedConfig, ConfigError>,
}

impl App {
    /// Detects the project root and resolves configuration.
    pub fn build(
        root_override: Option<&Path>,
        overrides: &ConfigOverrides,
    ) -> anyhow::Result<Self> {
        let fs = RealFs;
        let git = GitCli::default();
        let cwd = std::env::current_dir()
            .and_then(|d| d.canonicalize())
            .context("cannot determine the current directory")?;
        let root_override = root_override
            .map(|p| {
                let p = cwd.join(p);
                p.canonicalize()
                    .with_context(|| format!("project root {} does not exist", p.display()))
            })
            .transpose()?;
        let root = detect_root(&fs, &git, &cwd, root_override.as_deref())?;
        let layout = StateLayout::new(&root.path);
        let config = agentos_config::resolve(
            &fs,
            &layout.config_file(),
            &EnvVars::from_process(),
            overrides,
        );
        Ok(Self {
            fs,
            git,
            root,
            layout,
            config,
        })
    }

    /// Log filter to use: the resolved config, or CLI/default values if config is invalid.
    pub fn log_filter(&self, overrides: &ConfigOverrides) -> String {
        match &self.config {
            Ok(resolved) => resolved.config.log_level.clone(),
            Err(_) => overrides
                .log_level
                .clone()
                .unwrap_or_else(|| Config::default().log_level),
        }
    }

    /// Output format to use: the resolved config, or CLI/default values if config is invalid.
    pub fn output_format(&self, overrides: &ConfigOverrides) -> OutputFormat {
        match &self.config {
            Ok(resolved) => resolved.config.output_format,
            Err(_) => overrides.output_format.unwrap_or_default(),
        }
    }
}
