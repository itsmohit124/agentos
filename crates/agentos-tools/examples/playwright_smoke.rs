//! Opt-in Playwright smoke check. This is not part of `cargo test`.
//!
//! ```text
//! cargo run -p agentos-tools --example playwright_smoke --offline --features playwright
//! ```
//!
//! A missing Node binary, `playwright` package, or Chromium executable prints `SKIPPED:` and
//! exits 0. That exit is not a pass. Open or inspect failures exit 1.

use std::io::{self, Write};
use std::process::ExitCode;

use agentos_tools::PlaywrightBrowser;

fn main() -> ExitCode {
    match PlaywrightBrowser::installed() {
        Err(reason) => {
            let _ = writeln!(io::stdout(), "SKIPPED: {reason}");
            ExitCode::SUCCESS
        }
        Ok(()) => match PlaywrightBrowser::new().smoke_open_and_inspect() {
            Ok(()) => {
                let _ = writeln!(io::stdout(), "PASSED: open inspect");
                ExitCode::SUCCESS
            }
            Err(err) => {
                let _ = writeln!(io::stderr(), "{err}");
                ExitCode::from(1)
            }
        },
    }
}
