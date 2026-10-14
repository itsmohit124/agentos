//! Feedback engine: validation providers and architecture analysis.
//!
//! The public seam is [`ValidationProvider`]. Concrete providers own command lines and parsers.
//! Process execution goes through [`agentos_tools`]. Architecture analysis reads the project
//! knowledge graph; it does not create another one.
//!
//! Browser validation is a [`ValidationProvider`] with kind `browser`. The default capability is
//! [`agentos_tools::NullBrowser`]. A project without `expected/ui.json` does not select it.

mod architecture;
mod config;
mod error;
mod kind;
mod parseutil;
mod provider;
mod providers;
mod result;
mod run;
mod store;

pub use architecture::{
    ArchitectureFinding, ArchitectureReport, GraphSnapshot, analyze, analyze_project,
};
pub use config::{
    ArchitectureConfig, DEFAULT_FAT_CONTROLLER_METHODS, DEFAULT_MAX_OUTPUT_BYTES,
    DEFAULT_PUBLIC_SURFACE_MAX, DEFAULT_TIMEOUT_SECS, ProjectConfig, ValidationConfig,
    load_project_config,
};
pub use error::FeedbackError;
pub use kind::ValidationKind;
pub use provider::{CommandPlan, RunRequest, ValidationProvider};
pub use providers::{
    BrowserProvider, CargoTestProvider, GoTestProvider, GradleProvider, JestProvider,
    MavenProvider, PytestProvider, RSpecProvider, builtin_providers, provider_by_id,
};
pub use result::{
    CapturedOutput, Diagnostic, FeedbackItem, Outcome, ResultProvenance, ValidationResult,
};
pub use run::{
    DetectedProvider, ExpectedDiagnostic, RegressionCase, check_regression_case, detect, last,
    record_regression, run_and_store,
};

/// Name of this crate.
pub const CRATE_NAME: &str = env!("CARGO_PKG_NAME");

#[cfg(test)]
mod tests;
