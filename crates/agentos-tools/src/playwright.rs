//! Optional Playwright session driven by a Node helper.
//!
//! This module is compiled only with `--features playwright`. The helper is a script embedded
//! in the binary. Node, the `playwright` package, and the Chromium binary are required at
//! runtime, not at compile time.
//!
//! The real browser smoke check is not a unit test. Run it explicitly:
//!
//! ```text
//! cargo run -p agentos-tools --example playwright_smoke --offline --features playwright
//! ```
//!
//! A missing package or browser prints `SKIPPED:` and exits 0. That exit is not a pass.

use std::fmt;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::Duration;

use serde::Deserialize;

use crate::browser::{BrowserCapability, BrowserStatus, BrowserStep, Inspection, NetworkExchange};

const HELPER: &str = include_str!("playwright_helper.cjs");
const COMMAND_TIMEOUT: Duration = Duration::from_secs(60);
const INSTALL_CHECK: &str = r#"
const fs = require("fs");
let playwright;
try {
  playwright = require("playwright");
} catch (err) {
  const code = err && err.code ? err.code + ": " : "";
  process.stderr.write(code + err.message + "\n");
  process.exit(1);
}
const executable = playwright.chromium.executablePath();
if (!fs.existsSync(executable)) {
  process.stderr.write("chromium executable is missing: " + executable + "\n");
  process.exit(2);
}
"#;
const SMOKE_URL: &str =
    "data:text/html,<!DOCTYPE html><html><body><h1 id=\"title\">agentos</h1></body></html>";

/// Local Chromium session. A missing Node or Playwright package reports unavailable.
pub struct PlaywrightBrowser {
    session: Option<Session>,
}

impl fmt::Debug for PlaywrightBrowser {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PlaywrightBrowser")
            .field("running", &self.session.is_some())
            .finish()
    }
}

impl Default for PlaywrightBrowser {
    fn default() -> Self {
        Self::new()
    }
}

impl PlaywrightBrowser {
    /// A session that starts the helper on the first action.
    pub fn new() -> Self {
        Self { session: None }
    }

    /// `node` can load `playwright` and the Chromium executable is on disk.
    ///
    /// This does not launch a browser. `Err` is the skip reason, such as `MODULE_NOT_FOUND` or
    /// a missing Chromium path.
    pub fn installed() -> Result<(), String> {
        let output = Command::new("node")
            .arg("-e")
            .arg(INSTALL_CHECK)
            .output()
            .map_err(|err| format!("node is not available: {err}"))?;
        if output.status.success() {
            return Ok(());
        }
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let detail = if stderr.trim().is_empty() {
            stdout.trim()
        } else {
            stderr.trim()
        };
        if detail.is_empty() {
            Err(format!(
                "playwright preflight exited {}",
                output.status.code().unwrap_or(-1)
            ))
        } else {
            Err(detail.to_owned())
        }
    }

    /// Opens a local page and checks that inspect reports `h1#title=agentos`.
    ///
    /// Call this only after [`Self::installed`] succeeds. A missing package is reported by that
    /// check, not by a passing result here.
    pub fn smoke_open_and_inspect(&mut self) -> Result<(), String> {
        let opened = self.open(SMOKE_URL);
        if opened.status != BrowserStatus::Completed {
            return Err(format!("playwright open failed: {}", opened.detail));
        }
        let inspected = self.inspect();
        if inspected.status != BrowserStatus::Completed {
            return Err(format!("playwright inspect failed: {}", inspected.detail));
        }
        let inspection = inspected
            .inspection
            .ok_or_else(|| "playwright inspect returned no inspection".to_owned())?;
        if inspection.dom.iter().any(|line| line == "h1#title=agentos") {
            Ok(())
        } else {
            Err(format!(
                "dom {:?} does not contain h1#title=agentos",
                inspection.dom
            ))
        }
    }
}

impl BrowserCapability for PlaywrightBrowser {
    fn open(&mut self, url: &str) -> BrowserStep {
        self.round_trip(HelperCommand::Open { url })
    }

    fn click(&mut self, selector: &str) -> BrowserStep {
        self.round_trip(HelperCommand::Click { selector })
    }

    fn fill(&mut self, selector: &str, value: &str) -> BrowserStep {
        self.round_trip(HelperCommand::Fill { selector, value })
    }

    fn screenshot(&mut self, destination: &Path) -> BrowserStep {
        self.round_trip(HelperCommand::Screenshot {
            path: &destination.display().to_string(),
        })
    }

    fn inspect(&mut self) -> BrowserStep {
        self.round_trip(HelperCommand::Inspect)
    }
}

impl Drop for PlaywrightBrowser {
    fn drop(&mut self) {
        if let Some(mut session) = self.session.take() {
            let _ = session.send(HelperCommand::Close);
            let _ = session.child.kill();
            let _ = session.child.wait();
        }
    }
}

enum HelperCommand<'a> {
    Open { url: &'a str },
    Click { selector: &'a str },
    Fill { selector: &'a str, value: &'a str },
    Screenshot { path: &'a str },
    Inspect,
    Close,
}

impl HelperCommand<'_> {
    fn json(&self) -> String {
        match self {
            Self::Open { url } => serde_json::json!({"action": "open", "url": url}).to_string(),
            Self::Click { selector } => {
                serde_json::json!({"action": "click", "selector": selector}).to_string()
            }
            Self::Fill { selector, value } => {
                serde_json::json!({"action": "fill", "selector": selector, "value": value})
                    .to_string()
            }
            Self::Screenshot { path } => {
                serde_json::json!({"action": "screenshot", "path": path}).to_string()
            }
            Self::Inspect => serde_json::json!({"action": "inspect"}).to_string(),
            Self::Close => serde_json::json!({"action": "close"}).to_string(),
        }
    }
}

struct Session {
    child: Child,
    stdin: std::process::ChildStdin,
    lines: std::sync::mpsc::Receiver<std::io::Result<String>>,
}

impl PlaywrightBrowser {
    fn round_trip(&mut self, command: HelperCommand<'_>) -> BrowserStep {
        let action = match command {
            HelperCommand::Open { .. } => "open",
            HelperCommand::Click { .. } => "click",
            HelperCommand::Fill { .. } => "fill",
            HelperCommand::Screenshot { .. } => "screenshot",
            HelperCommand::Inspect => "inspect",
            HelperCommand::Close => "close",
        };
        if self.session.is_none() {
            match Session::start() {
                Ok(session) => self.session = Some(session),
                Err(detail) => return unavailable(action, &detail),
            }
        }
        let Some(session) = self.session.as_mut() else {
            return unavailable(action, "playwright session is not running");
        };
        match session.send(command) {
            Ok(step) => step,
            Err(detail) => {
                self.session = None;
                unavailable(action, &detail)
            }
        }
    }
}

impl Session {
    fn start() -> Result<Self, String> {
        let dir = std::env::temp_dir().join(format!("agentos-playwright-{}", std::process::id()));
        std::fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
        let script = dir.join("helper.cjs");
        std::fs::write(&script, HELPER).map_err(|err| err.to_string())?;
        let mut child = Command::new("node")
            .arg(&script)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|err| format!("node is not available: {err}"))?;
        let stdin = child.stdin.take().ok_or("playwright helper has no stdin")?;
        let stdout = child
            .stdout
            .take()
            .ok_or("playwright helper has no stdout")?;
        let stderr = child.stderr.take();
        let (tx, lines) = std::sync::mpsc::channel();
        thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) => break,
                    Ok(_) => {
                        if tx.send(Ok(line)).is_err() {
                            break;
                        }
                    }
                    Err(err) => {
                        let _ = tx.send(Err(err));
                        break;
                    }
                }
            }
        });
        if let Some(stderr) = stderr {
            thread::spawn(move || {
                let mut reader = BufReader::new(stderr);
                let mut sink = String::new();
                let _ = reader.read_to_string(&mut sink);
            });
        }
        Ok(Self {
            child,
            stdin,
            lines,
        })
    }

    fn send(&mut self, command: HelperCommand<'_>) -> Result<BrowserStep, String> {
        let line = command.json();
        writeln!(self.stdin, "{line}").map_err(|err| err.to_string())?;
        self.stdin.flush().map_err(|err| err.to_string())?;
        let received = self
            .lines
            .recv_timeout(COMMAND_TIMEOUT)
            .map_err(|_| "playwright helper timed out".to_owned())?
            .map_err(|err| err.to_string())?;
        let parsed: HelperResponse =
            serde_json::from_str(received.trim()).map_err(|err| err.to_string())?;
        Ok(parsed.into_step())
    }
}

#[derive(Debug, Deserialize)]
struct HelperResponse {
    action: String,
    status: String,
    detail: String,
    inspection: Option<HelperInspection>,
    artifact: Option<String>,
}

#[derive(Debug, Deserialize)]
struct HelperInspection {
    dom: Vec<String>,
    console: Vec<String>,
    network: Vec<HelperNetwork>,
}

#[derive(Debug, Deserialize)]
struct HelperNetwork {
    method: String,
    url: String,
}

impl HelperResponse {
    fn into_step(self) -> BrowserStep {
        let status = match self.status.as_str() {
            "completed" => BrowserStatus::Completed,
            "unavailable" => BrowserStatus::Unavailable,
            _ => BrowserStatus::Failed,
        };
        let inspection = self.inspection.map(|item| Inspection {
            dom: item.dom,
            console: item.console,
            network: item
                .network
                .into_iter()
                .map(|entry| NetworkExchange {
                    method: entry.method,
                    url: entry.url,
                })
                .collect(),
        });
        if status != BrowserStatus::Completed && inspection.is_some() {
            return BrowserStep {
                action: self.action,
                status,
                detail: self.detail,
                inspection: None,
                artifact: None,
            };
        }
        BrowserStep {
            action: self.action,
            status,
            detail: self.detail,
            inspection,
            artifact: self.artifact,
        }
    }
}

fn unavailable(action: &str, detail: &str) -> BrowserStep {
    BrowserStep {
        action: action.to_owned(),
        status: BrowserStatus::Unavailable,
        detail: format!("browser unavailable; {detail}"),
        inspection: None,
        artifact: None,
    }
}
