//! The CLI output layer: the only place that writes to stdout/stderr directly.

use std::io::Write;

use anyhow::Context;
use serde::Serialize;

/// Writes `text` to stdout.
pub fn text(text: &str) -> anyhow::Result<()> {
    let mut out = std::io::stdout().lock();
    out.write_all(text.as_bytes())
        .and_then(|()| out.flush())
        .context("cannot write to stdout")
}

/// Writes `value` to stdout as pretty JSON followed by a newline.
pub fn json<T: Serialize>(value: &T) -> anyhow::Result<()> {
    let mut rendered = serde_json::to_string_pretty(value).context("cannot serialize output")?;
    rendered.push('\n');
    text(&rendered)
}

/// Writes a warning to stderr.
#[allow(clippy::print_stderr)]
pub fn warn(message: &str) {
    eprintln!("warning: {message}");
}

/// Writes an error and its cause chain to stderr.
#[allow(clippy::print_stderr)]
pub fn error(err: &anyhow::Error) {
    eprintln!("error: {err:#}");
}
