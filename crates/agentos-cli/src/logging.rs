//! Structured logging setup. Logs go to stderr so stdout stays machine-readable.

use std::io::IsTerminal;

use tracing_subscriber::EnvFilter;

/// Installs the global subscriber with `filter`, falling back to `warn` if it cannot be parsed.
pub fn init(filter: String) {
    let env_filter = EnvFilter::try_new(&filter).unwrap_or_else(|_| EnvFilter::new("warn"));
    let ansi = std::io::stderr().is_terminal() && std::env::var_os("NO_COLOR").is_none();
    let _ = tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_writer(std::io::stderr)
        .with_ansi(ansi)
        .try_init();
}
