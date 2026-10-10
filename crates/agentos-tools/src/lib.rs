//! Process execution behind the permission check.
//!
//! Callers describe a command. [`ProcessRunner`] refuses it when the subject did not declare
//! `shell`, then enforces the timeout and the stdout/stderr byte cap. Providers must not spawn
//! processes themselves.

mod error;
mod exec;

pub use error::ToolError;
pub use exec::{ExecOutcome, ExecRequest, ProcessRunner, ToolRunner};

/// Name of this crate.
pub const CRATE_NAME: &str = env!("CARGO_PKG_NAME");

#[cfg(test)]
mod tests {
    #[test]
    fn smoke() {
        assert_eq!(super::CRATE_NAME, "agentos-tools");
    }
}
