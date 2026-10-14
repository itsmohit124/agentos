//! Process execution and the browser capability, both behind a permission check.
//!
//! Callers describe a command. [`ProcessRunner`] refuses it when the subject did not declare
//! `shell`, then enforces the timeout and the stdout/stderr byte cap. Providers must not spawn
//! processes themselves.
//!
//! [`BrowserTool`] refuses browser actions when the subject did not declare `browser`. The
//! default capability is [`NullBrowser`], which reports unavailable and does not open a page.

mod browser;
mod error;
mod exec;

#[cfg(feature = "playwright")]
mod playwright;

pub use browser::{
    BROWSER_RUN_ID, BrowserCapability, BrowserStatus, BrowserStep, BrowserTool, CHECKOUT_CLICK,
    CHECKOUT_FIELD, CHECKOUT_URL, CHECKOUT_VALUE, FAKE_SCREENSHOT, FakeBrowser, Inspection,
    NetworkExchange, NullBrowser, artifact_path, prepare_run_artifacts,
};
pub use error::ToolError;
pub use exec::{ExecOutcome, ExecRequest, ProcessRunner, ToolRunner};

#[cfg(feature = "playwright")]
pub use playwright::PlaywrightBrowser;

/// Name of this crate.
pub const CRATE_NAME: &str = env!("CARGO_PKG_NAME");

#[cfg(test)]
mod tests {
    #[test]
    fn smoke() {
        assert_eq!(super::CRATE_NAME, "agentos-tools");
    }
}
