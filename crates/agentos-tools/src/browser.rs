//! Browser capability behind the `browser` permission.
//!
//! [`BrowserTool`] is the enforcement point: it calls [`agentos_permissions::check`] for
//! [`Capability::Browser`] and only then invokes a [`BrowserCapability`]. [`NullBrowser`] is the
//! default. It reports unavailable and does not record a page. [`FakeBrowser`] replays a
//! deterministic checkout scenario for tests.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

use agentos_permissions::{Capability, PermissionSubject, check};

use crate::error::ToolError;

/// Stable run id for one project's browser artifacts.
pub const BROWSER_RUN_ID: &str = "browser-ui";

/// Checkout page used by [`FakeBrowser`] and the TypeScript React fixture.
pub const CHECKOUT_URL: &str = "http://payments.local/checkout";
/// Amount field filled in the checkout scenario.
pub const CHECKOUT_FIELD: &str = "#amount";
/// Value written into [`CHECKOUT_FIELD`].
pub const CHECKOUT_VALUE: &str = "42";
/// Control clicked after the amount is filled.
pub const CHECKOUT_CLICK: &str = "#charge";
/// Bytes [`FakeBrowser`] writes for a screenshot. No pixels and no timestamp.
pub const FAKE_SCREENSHOT: &[u8] = b"agentos-fake-screenshot\n";

/// How one browser action finished.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserStatus {
    /// The action ran against the capability's page model.
    Completed,
    /// No browser was available. The page was not opened or changed.
    Unavailable,
    /// The action was attempted and rejected by the page model.
    Failed,
}

/// One network exchange reported by [`Inspection`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkExchange {
    /// HTTP method, uppercase.
    pub method: String,
    /// Request URL.
    pub url: String,
}

/// DOM, console, and network summaries. Lists are sorted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inspection {
    /// DOM lines such as `input#amount=42`.
    pub dom: Vec<String>,
    /// Console messages.
    pub console: Vec<String>,
    /// Network exchanges, sorted by method then URL.
    pub network: Vec<NetworkExchange>,
}

/// Result of one browser action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserStep {
    /// `open`, `click`, `fill`, `screenshot`, or `inspect`.
    pub action: String,
    /// Whether the capability performed the action.
    pub status: BrowserStatus,
    /// Stable explanation. A null browser never includes the requested URL.
    pub detail: String,
    /// Present only for a completed `inspect`.
    pub inspection: Option<Inspection>,
    /// Relative or absolute artifact path when a screenshot was written.
    pub artifact: Option<String>,
}

/// Drives a page. Implementations do not check permissions; [`BrowserTool`] does.
pub trait BrowserCapability {
    /// Opens `url`.
    fn open(&mut self, url: &str) -> BrowserStep;
    /// Clicks `selector`.
    fn click(&mut self, selector: &str) -> BrowserStep;
    /// Fills `selector` with `value`.
    fn fill(&mut self, selector: &str, value: &str) -> BrowserStep;
    /// Writes a screenshot to `destination`.
    fn screenshot(&mut self, destination: &Path) -> BrowserStep;
    /// Summarises the DOM, console, and network activity.
    fn inspect(&mut self) -> BrowserStep;
}

/// Permission gate in front of a [`BrowserCapability`].
#[derive(Debug)]
pub struct BrowserTool<B> {
    inner: B,
}

impl<B> BrowserTool<B> {
    /// Wraps `inner`. No capability is granted by constructing the tool.
    pub fn new(inner: B) -> Self {
        Self { inner }
    }

    /// Borrows the capability. A denied call leaves this unchanged.
    pub fn inner(&self) -> &B {
        &self.inner
    }
}

impl<B: BrowserCapability> BrowserTool<B> {
    /// Opens `url` when `subject` declares `browser`.
    pub fn open(
        &mut self,
        subject: &dyn PermissionSubject,
        url: &str,
    ) -> Result<BrowserStep, ToolError> {
        self.authorize(subject)?;
        Ok(self.inner.open(url))
    }

    /// Clicks `selector` when `subject` declares `browser`.
    pub fn click(
        &mut self,
        subject: &dyn PermissionSubject,
        selector: &str,
    ) -> Result<BrowserStep, ToolError> {
        self.authorize(subject)?;
        Ok(self.inner.click(selector))
    }

    /// Fills `selector` when `subject` declares `browser`.
    pub fn fill(
        &mut self,
        subject: &dyn PermissionSubject,
        selector: &str,
        value: &str,
    ) -> Result<BrowserStep, ToolError> {
        self.authorize(subject)?;
        Ok(self.inner.fill(selector, value))
    }

    /// Writes `file_name` under `.agentos/runs/<run_id>/artifacts/` when `subject` declares
    /// `browser`. A path that leaves that directory is rejected before the capability runs.
    pub fn screenshot(
        &mut self,
        subject: &dyn PermissionSubject,
        root: &Path,
        run_id: &str,
        file_name: &str,
    ) -> Result<BrowserStep, ToolError> {
        self.authorize(subject)?;
        let path = artifact_path(root, run_id, file_name)?;
        guard_existing(root, run_id)?;
        let step = self.inner.screenshot(&path);
        if step.status != BrowserStatus::Completed {
            if path.exists() {
                let _ = fs::remove_file(&path);
            }
            return Ok(step);
        }
        ensure_file_contained(root, run_id, &path)?;
        Ok(step)
    }

    /// Inspects the page when `subject` declares `browser`.
    pub fn inspect(&mut self, subject: &dyn PermissionSubject) -> Result<BrowserStep, ToolError> {
        self.authorize(subject)?;
        Ok(self.inner.inspect())
    }

    fn authorize(&self, subject: &dyn PermissionSubject) -> Result<(), ToolError> {
        let decision = check(subject, Capability::Browser);
        if decision.is_allowed() {
            Ok(())
        } else {
            tracing::info!(subject = %subject.subject(), "browser permission denied");
            Err(ToolError::Denied {
                reason: decision.reason,
                capability: Capability::Browser,
            })
        }
    }
}

/// Default browser. Every action is unavailable and no file is written.
#[derive(Debug, Default, Clone, Copy)]
pub struct NullBrowser;

impl BrowserCapability for NullBrowser {
    fn open(&mut self, _url: &str) -> BrowserStep {
        unavailable("open")
    }

    fn click(&mut self, _selector: &str) -> BrowserStep {
        unavailable("click")
    }

    fn fill(&mut self, _selector: &str, _value: &str) -> BrowserStep {
        unavailable("fill")
    }

    fn screenshot(&mut self, _destination: &Path) -> BrowserStep {
        unavailable("screenshot")
    }

    fn inspect(&mut self) -> BrowserStep {
        unavailable("inspect")
    }
}

fn unavailable(action: &str) -> BrowserStep {
    BrowserStep {
        action: action.to_owned(),
        status: BrowserStatus::Unavailable,
        detail: "browser unavailable; no page was opened or changed".to_owned(),
        inspection: None,
        artifact: None,
    }
}

/// Deterministic page model for tests.
///
/// `open`, then `fill`, then `click`, then `inspect`. A click on `#charge` records the console
/// line `charge submitted` and `POST {origin}/charge`. DOM lines are `input{selector}={value}`
/// and `button{selector}=clicked`, sorted.
#[derive(Debug, Default)]
pub struct FakeBrowser {
    opened: Option<String>,
    fields: BTreeMap<String, String>,
    clicks: BTreeSet<String>,
    calls: Vec<String>,
}

impl FakeBrowser {
    /// An empty page. Nothing is open.
    pub fn new() -> Self {
        Self::default()
    }

    /// Actions that actually reached the page model, in order.
    pub fn calls(&self) -> &[String] {
        &self.calls
    }
}

impl BrowserCapability for FakeBrowser {
    fn open(&mut self, url: &str) -> BrowserStep {
        self.calls.push("open".to_owned());
        if url.is_empty() || origin_of(url).is_none() {
            return failed("open", "url is empty");
        }
        self.opened = Some(url.to_owned());
        self.fields.clear();
        self.clicks.clear();
        BrowserStep {
            action: "open".to_owned(),
            status: BrowserStatus::Completed,
            detail: format!("opened {url}"),
            inspection: None,
            artifact: None,
        }
    }

    fn fill(&mut self, selector: &str, value: &str) -> BrowserStep {
        self.calls.push("fill".to_owned());
        if self.opened.is_none() {
            return failed("fill", "no page is open");
        }
        if selector.is_empty() {
            return failed("fill", "selector is empty");
        }
        self.fields.insert(selector.to_owned(), value.to_owned());
        BrowserStep {
            action: "fill".to_owned(),
            status: BrowserStatus::Completed,
            detail: format!("filled {selector}"),
            inspection: None,
            artifact: None,
        }
    }

    fn click(&mut self, selector: &str) -> BrowserStep {
        self.calls.push("click".to_owned());
        if self.opened.is_none() {
            return failed("click", "no page is open");
        }
        if self.fields.is_empty() {
            return failed("click", "no field has been filled");
        }
        if selector.is_empty() {
            return failed("click", "selector is empty");
        }
        self.clicks.insert(selector.to_owned());
        BrowserStep {
            action: "click".to_owned(),
            status: BrowserStatus::Completed,
            detail: format!("clicked {selector}"),
            inspection: None,
            artifact: None,
        }
    }

    fn screenshot(&mut self, destination: &Path) -> BrowserStep {
        self.calls.push("screenshot".to_owned());
        if self.opened.is_none() {
            return failed("screenshot", "no page is open");
        }
        if let Some(parent) = destination.parent()
            && !parent.as_os_str().is_empty()
            && let Err(err) = fs::create_dir_all(parent)
        {
            return failed("screenshot", &err.to_string());
        }
        if let Err(err) = fs::write(destination, FAKE_SCREENSHOT) {
            return failed("screenshot", &err.to_string());
        }
        BrowserStep {
            action: "screenshot".to_owned(),
            status: BrowserStatus::Completed,
            detail: "wrote screenshot".to_owned(),
            inspection: None,
            artifact: Some(destination.display().to_string()),
        }
    }

    fn inspect(&mut self) -> BrowserStep {
        self.calls.push("inspect".to_owned());
        let Some(url) = self.opened.clone() else {
            return failed("inspect", "no page is open");
        };
        if self.clicks.is_empty() {
            return failed("inspect", "no control has been clicked");
        }
        let mut dom = Vec::new();
        for (selector, value) in &self.fields {
            dom.push(format!("input{selector}={value}"));
        }
        for selector in &self.clicks {
            dom.push(format!("button{selector}=clicked"));
        }
        dom.sort();
        let mut console = Vec::new();
        let mut network = Vec::new();
        if self.clicks.contains(CHECKOUT_CLICK)
            && let Some(origin) = origin_of(&url)
        {
            console.push("charge submitted".to_owned());
            network.push(NetworkExchange {
                method: "POST".to_owned(),
                url: format!("{origin}/charge"),
            });
        }
        network.sort_by(|left, right| (&left.method, &left.url).cmp(&(&right.method, &right.url)));
        BrowserStep {
            action: "inspect".to_owned(),
            status: BrowserStatus::Completed,
            detail: format!("inspected {url}"),
            inspection: Some(Inspection {
                dom,
                console,
                network,
            }),
            artifact: None,
        }
    }
}

fn failed(action: &str, detail: &str) -> BrowserStep {
    BrowserStep {
        action: action.to_owned(),
        status: BrowserStatus::Failed,
        detail: detail.to_owned(),
        inspection: None,
        artifact: None,
    }
}

fn origin_of(url: &str) -> Option<String> {
    let (scheme, rest) = if let Some(rest) = url.strip_prefix("https://") {
        ("https", rest)
    } else {
        ("http", url.strip_prefix("http://")?)
    };
    let host = rest.split('/').next().unwrap_or("");
    if host.is_empty() || host.contains(' ') {
        return None;
    }
    Some(format!("{scheme}://{host}"))
}

/// Lexical path of `.agentos/runs/<run_id>/artifacts/<file_name>`.
///
/// `run_id` and `file_name` must be a single safe path segment. `..` and absolute names are
/// rejected before any directory is created.
pub fn artifact_path(root: &Path, run_id: &str, file_name: &str) -> Result<PathBuf, ToolError> {
    if !safe_segment(run_id) {
        return Err(ToolError::Artifact(format!(
            "run id {run_id:?} is not a single path segment"
        )));
    }
    if !safe_segment(file_name) {
        return Err(ToolError::Artifact(format!(
            "artifact name {file_name:?} is not a single path segment"
        )));
    }
    Ok(root
        .join(".agentos")
        .join("runs")
        .join(run_id)
        .join("artifacts")
        .join(file_name))
}

/// Creates the artifact directory and returns its canonical path when it stays inside `root`.
pub fn prepare_run_artifacts(root: &Path, run_id: &str) -> Result<PathBuf, ToolError> {
    if !safe_segment(run_id) {
        return Err(ToolError::Artifact(format!(
            "run id {run_id:?} is not a single path segment"
        )));
    }
    let root = root
        .canonicalize()
        .map_err(|err| ToolError::Artifact(format!("cannot resolve project root: {err}")))?;
    let mut current = root.clone();
    for component in [".agentos", "runs", run_id, "artifacts"] {
        current.push(component);
        if current.exists() {
            let meta = fs::symlink_metadata(&current).map_err(ToolError::Io)?;
            if meta.file_type().is_symlink() {
                let target = current.canonicalize().map_err(ToolError::Io)?;
                if !target.starts_with(&root) {
                    return Err(ToolError::Artifact(format!(
                        "{} resolves outside the project",
                        current.display()
                    )));
                }
            }
        }
    }
    fs::create_dir_all(&current)?;
    let canon = current.canonicalize().map_err(ToolError::Io)?;
    if !canon.starts_with(&root) {
        return Err(ToolError::Artifact(format!(
            "{} resolves outside the project",
            canon.display()
        )));
    }
    Ok(canon)
}

fn guard_existing(root: &Path, run_id: &str) -> Result<(), ToolError> {
    if !safe_segment(run_id) {
        return Err(ToolError::Artifact(format!(
            "run id {run_id:?} is not a single path segment"
        )));
    }
    let root = root
        .canonicalize()
        .map_err(|err| ToolError::Artifact(format!("cannot resolve project root: {err}")))?;
    let mut current = root.clone();
    for component in [".agentos", "runs", run_id, "artifacts"] {
        current.push(component);
        if !current.exists() {
            return Ok(());
        }
        let meta = fs::symlink_metadata(&current).map_err(ToolError::Io)?;
        if meta.file_type().is_symlink() {
            let target = current.canonicalize().map_err(ToolError::Io)?;
            if !target.starts_with(&root) {
                return Err(ToolError::Artifact(format!(
                    "{} resolves outside the project",
                    current.display()
                )));
            }
        }
    }
    Ok(())
}

fn ensure_file_contained(root: &Path, run_id: &str, path: &Path) -> Result<(), ToolError> {
    let root = root.canonicalize().map_err(ToolError::Io)?;
    let file = path.canonicalize().map_err(ToolError::Io)?;
    let artifacts = root
        .join(".agentos")
        .join("runs")
        .join(run_id)
        .join("artifacts");
    let artifacts = artifacts.canonicalize().map_err(ToolError::Io)?;
    if !file.starts_with(&artifacts) || !artifacts.starts_with(&root) {
        let _ = fs::remove_file(&file);
        return Err(ToolError::Artifact(format!(
            "{} is outside {}",
            file.display(),
            artifacts.display()
        )));
    }
    Ok(())
}

fn safe_segment(value: &str) -> bool {
    if value.is_empty() || value == "." || value == ".." {
        return false;
    }
    let path = Path::new(value);
    if path.is_absolute() || path.components().count() != 1 {
        return false;
    }
    match path.components().next() {
        Some(Component::Normal(segment)) => segment.to_str().is_some_and(|text| {
            text.chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '.' || ch == '_' || ch == '-')
        }),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use agentos_permissions::{Capability, PermissionSet, PermissionSubject};
    use tempfile::tempdir;

    use super::*;

    struct Skill {
        name: &'static str,
        permissions: PermissionSet,
    }

    impl PermissionSubject for Skill {
        fn subject(&self) -> String {
            format!("skill '{}'", self.name)
        }
        fn permissions(&self) -> &PermissionSet {
            &self.permissions
        }
    }

    fn skill(name: &'static str, caps: &[Capability]) -> Skill {
        Skill {
            name,
            permissions: caps.iter().copied().collect(),
        }
    }

    #[test]
    fn missing_browser_permission_does_not_touch_the_page() {
        let subject = skill("ui-review", &[Capability::Shell, Capability::FsRead]);
        let mut tool = BrowserTool::new(FakeBrowser::new());
        let err = tool.open(&subject, CHECKOUT_URL).unwrap_err();
        match err {
            ToolError::Denied { capability, reason } => {
                assert_eq!(capability, Capability::Browser);
                assert_eq!(
                    reason,
                    "skill 'ui-review' does not declare browser (default deny)"
                );
            }
            other => panic!("expected denial, got {other}"),
        }
        assert!(tool.inner().calls().is_empty());
        assert!(tool.click(&subject, CHECKOUT_CLICK).is_err());
        assert!(tool.fill(&subject, CHECKOUT_FIELD, CHECKOUT_VALUE).is_err());
        assert!(tool.inspect(&subject).is_err());
        assert!(tool.inner().calls().is_empty());
    }

    #[test]
    fn shell_does_not_grant_browser() {
        let subject = skill("shell-only", &[Capability::Shell]);
        let mut tool = BrowserTool::new(FakeBrowser::new());
        assert!(tool.open(&subject, CHECKOUT_URL).is_err());
        assert!(tool.inner().calls().is_empty());
    }

    #[test]
    fn null_browser_reports_unavailable_and_does_not_record_the_url() {
        let subject = skill("ui-review", &[Capability::Browser]);
        let mut tool = BrowserTool::new(NullBrowser);
        let root = tempdir().unwrap();
        for step in [
            tool.open(&subject, CHECKOUT_URL).unwrap(),
            tool.fill(&subject, CHECKOUT_FIELD, CHECKOUT_VALUE).unwrap(),
            tool.click(&subject, CHECKOUT_CLICK).unwrap(),
            tool.inspect(&subject).unwrap(),
        ] {
            assert_eq!(step.status, BrowserStatus::Unavailable);
            assert!(step.inspection.is_none());
            assert!(step.artifact.is_none());
            assert_eq!(
                step.detail,
                "browser unavailable; no page was opened or changed"
            );
            assert!(!step.detail.contains(CHECKOUT_URL));
            assert!(!step.detail.contains(CHECKOUT_VALUE));
        }
        let shot = tool
            .screenshot(&subject, root.path(), BROWSER_RUN_ID, "page.bin")
            .unwrap();
        assert_eq!(shot.status, BrowserStatus::Unavailable);
        assert!(!root.path().join(".agentos/runs").exists());
    }

    #[test]
    fn fake_browser_runs_open_fill_click_inspect() {
        let subject = skill("ui-review", &[Capability::Browser]);
        let mut tool = BrowserTool::new(FakeBrowser::new());
        assert_eq!(
            tool.open(&subject, CHECKOUT_URL).unwrap().status,
            BrowserStatus::Completed
        );
        assert_eq!(
            tool.fill(&subject, CHECKOUT_FIELD, CHECKOUT_VALUE)
                .unwrap()
                .status,
            BrowserStatus::Completed
        );
        assert_eq!(
            tool.click(&subject, CHECKOUT_CLICK).unwrap().status,
            BrowserStatus::Completed
        );
        let inspected = tool.inspect(&subject).unwrap();
        assert_eq!(inspected.status, BrowserStatus::Completed);
        let inspection = inspected.inspection.expect("inspection");
        assert_eq!(
            inspection.dom,
            vec![
                "button#charge=clicked".to_owned(),
                "input#amount=42".to_owned(),
            ]
        );
        assert_eq!(inspection.console, vec!["charge submitted".to_owned()]);
        assert_eq!(
            inspection.network,
            vec![NetworkExchange {
                method: "POST".to_owned(),
                url: "http://payments.local/charge".to_owned(),
            }]
        );
        assert_eq!(tool.inner().calls(), ["open", "fill", "click", "inspect"]);
    }

    #[test]
    fn inspect_before_click_does_not_invent_a_result() {
        let subject = skill("ui-review", &[Capability::Browser]);
        let mut tool = BrowserTool::new(FakeBrowser::new());
        tool.open(&subject, CHECKOUT_URL).unwrap();
        let step = tool.inspect(&subject).unwrap();
        assert_eq!(step.status, BrowserStatus::Failed);
        assert!(step.inspection.is_none());
    }

    #[test]
    fn artifact_paths_stay_inside_the_run_directory() {
        let root = tempdir().unwrap();
        let path = artifact_path(root.path(), BROWSER_RUN_ID, "inspect.json").unwrap();
        assert_eq!(
            path,
            root.path()
                .join(".agentos/runs/browser-ui/artifacts/inspect.json")
        );
        assert!(artifact_path(root.path(), BROWSER_RUN_ID, "../secret.txt").is_err());
        assert!(artifact_path(root.path(), "../escape", "page.bin").is_err());
        assert!(artifact_path(root.path(), BROWSER_RUN_ID, "/tmp/page.bin").is_err());
        assert!(artifact_path(root.path(), "browser/ui", "page.bin").is_err());

        let subject = skill("ui-review", &[Capability::Browser]);
        let mut tool = BrowserTool::new(FakeBrowser::new());
        tool.open(&subject, CHECKOUT_URL).unwrap();
        let shot = tool
            .screenshot(&subject, root.path(), BROWSER_RUN_ID, "page.bin")
            .unwrap();
        assert_eq!(shot.status, BrowserStatus::Completed);
        let written = root
            .path()
            .join(".agentos/runs/browser-ui/artifacts/page.bin");
        assert_eq!(fs::read(&written).unwrap(), FAKE_SCREENSHOT);
        let canon = written.canonicalize().unwrap();
        let artifacts = root
            .path()
            .join(".agentos/runs/browser-ui/artifacts")
            .canonicalize()
            .unwrap();
        assert!(canon.starts_with(&artifacts));
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_outside_the_project_is_rejected() {
        let root = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let run = root.path().join(".agentos/runs").join(BROWSER_RUN_ID);
        fs::create_dir_all(&run).unwrap();
        std::os::unix::fs::symlink(outside.path(), run.join("artifacts")).unwrap();
        let subject = skill("ui-review", &[Capability::Browser]);
        let mut tool = BrowserTool::new(FakeBrowser::new());
        tool.open(&subject, CHECKOUT_URL).unwrap();
        let err = tool
            .screenshot(&subject, root.path(), BROWSER_RUN_ID, "page.bin")
            .unwrap_err();
        assert!(matches!(err, ToolError::Artifact(_)), "{err}");
        assert!(!outside.path().join("page.bin").exists());
    }
}
