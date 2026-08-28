//! Capability-registry behaviour, exercised against stub ports.
//!
//! The stubs are the point as much as the assertions: if a fake tracker that
//! stores issues in a `Vec` can satisfy `IssueTracker` and drive every
//! `issues.*` capability unchanged, then the port abstraction is real and
//! swapping beads for Linear later (plan §2) is an adapter change and nothing
//! more.

use aios_caps::ports::{IssueTracker, Knowledge, Vcs};
use aios_caps::{Capabilities, Context, Ports};
use aios_core::{Error, Result};
use aios_types::{
    Commit, Issue, IssueQuery, IssueStatus, NewIssue, Note, NoteHit, NoteRef, RepoStatus, Scope,
    WriteNote,
};
use serde_json::json;
use std::path::Path;
use std::sync::Mutex;

struct FakeTracker {
    issues: Mutex<Vec<Issue>>,
}

impl FakeTracker {
    fn new() -> Self {
        Self {
            issues: Mutex::new(Vec::new()),
        }
    }
}

impl IssueTracker for FakeTracker {
    fn backend(&self) -> &'static str {
        "fake"
    }
    fn available(&self, _repo: &Path) -> bool {
        true
    }
    fn list(&self, _repo: &Path, query: &IssueQuery) -> Result<Vec<Issue>> {
        let all = self.issues.lock().unwrap().clone();
        Ok(all
            .into_iter()
            .filter(|i| query.status.is_empty() || query.status.contains(&i.status))
            .collect())
    }
    fn ready(&self, _repo: &Path) -> Result<Vec<Issue>> {
        Ok(self
            .issues
            .lock()
            .unwrap()
            .iter()
            .filter(|i| i.blocked_by.is_empty() && i.status == IssueStatus::Open)
            .cloned()
            .collect())
    }
    fn get(&self, _repo: &Path, id: &str) -> Result<Issue> {
        self.issues
            .lock()
            .unwrap()
            .iter()
            .find(|i| i.id == id)
            .cloned()
            .ok_or_else(|| Error::ProjectNotFound(id.into()))
    }
    fn create(&self, _repo: &Path, new: &NewIssue) -> Result<Issue> {
        let mut issues = self.issues.lock().unwrap();
        let issue = Issue {
            id: format!("fake-{}", issues.len() + 1),
            title: new.title.clone(),
            status: IssueStatus::Open,
            priority: new.priority.unwrap_or(2),
            issue_type: new.issue_type.clone().unwrap_or_else(|| "task".into()),
            description: new.description.clone(),
            labels: new.labels.clone(),
            assignee: None,
            blocked_by: Vec::new(),
            created_at: None,
            updated_at: None,
        };
        issues.push(issue.clone());
        Ok(issue)
    }
    fn close(&self, _repo: &Path, id: &str, _reason: Option<&str>) -> Result<Issue> {
        let mut issues = self.issues.lock().unwrap();
        let issue = issues
            .iter_mut()
            .find(|i| i.id == id)
            .ok_or_else(|| Error::ProjectNotFound(id.into()))?;
        issue.status = IssueStatus::Closed;
        Ok(issue.clone())
    }
}

struct NoKnowledge;
impl Knowledge for NoKnowledge {
    fn backend(&self) -> &'static str {
        "none"
    }
    fn available(&self) -> bool {
        false
    }
    fn list(&self, _s: &Scope) -> Result<Vec<NoteRef>> {
        Err(Error::NoVault("stub".into()))
    }
    fn search(&self, _s: &Scope, _q: &str, _l: usize) -> Result<Vec<NoteHit>> {
        Err(Error::NoVault("stub".into()))
    }
    fn read(&self, _p: &str) -> Result<Note> {
        Err(Error::NoVault("stub".into()))
    }
    fn write(&self, _r: &WriteNote) -> Result<Note> {
        Err(Error::NoVault("stub".into()))
    }
}

struct NoVcs;
impl Vcs for NoVcs {
    fn backend(&self) -> &'static str {
        "none"
    }
    fn status(&self, _r: &Path) -> Result<RepoStatus> {
        Err(Error::ToolMissing { tool: "git".into() })
    }
    fn log(&self, _r: &Path, _l: usize) -> Result<Vec<Commit>> {
        Err(Error::ToolMissing { tool: "git".into() })
    }
    fn diff(
        &self,
        _r: &Path,
        _base: Option<&str>,
        _staged: bool,
        _max: usize,
    ) -> Result<aios_types::Diff> {
        Err(Error::ToolMissing { tool: "git".into() })
    }
}

fn context() -> Context {
    Context::new(
        aios_core::Config::default(),
        aios_core::Registry::at({
            let root = std::env::temp_dir().join(format!("aios-caps-test-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            root
        }),
        Ports {
            issues: Box::new(FakeTracker::new()),
            knowledge: Box::new(NoKnowledge),
            vcs: Box::new(NoVcs),
        },
    )
}

#[test]
fn every_capability_has_a_unique_dotted_name() {
    let caps = Capabilities::all();
    let mut names: Vec<_> = caps.iter().map(|c| c.name).collect();
    let total = names.len();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), total, "duplicate capability name");

    for c in caps.iter() {
        assert!(
            c.name.split('.').count() == 2,
            "{} should be `group.operation`",
            c.name
        );
        assert!(!c.summary.is_empty(), "{} has no summary", c.name);
    }
}

#[test]
fn write_capabilities_are_classified_as_writes() {
    // Effect drives MCP annotations, read-only agent profiles, and (from phase
    // 3) whether a call needs an approval, so a misclassification is a security
    // bug rather than a cosmetic one.
    let caps = Capabilities::all();
    for name in [
        "issues.create",
        "issues.close",
        "kb.write",
        "kb.capture",
        "projects.add",
    ] {
        assert!(
            caps.get(name).unwrap().effect.is_write(),
            "{name} must be a write"
        );
    }
    for name in [
        "issues.list",
        "issues.ready",
        "kb.search",
        "vcs.status",
        "projects.list",
    ] {
        assert!(
            !caps.get(name).unwrap().effect.is_write(),
            "{name} must be a read"
        );
    }
}

#[test]
fn dispatches_by_name_through_json() {
    let caps = Capabilities::all();
    let ctx = context();

    let created = caps
        .call(
            &ctx,
            "issues.create",
            json!({ "title": "from json", "priority": 1 }),
        )
        .unwrap();
    assert_eq!(created["id"], "fake-1");
    assert_eq!(created["status"], "open");

    let listed = caps.call(&ctx, "issues.list", json!({})).unwrap();
    assert_eq!(listed.as_array().unwrap().len(), 1);

    let closed = caps
        .call(&ctx, "issues.close", json!({ "id": "fake-1" }))
        .unwrap();
    assert_eq!(closed["status"], "closed");
}

#[test]
fn rejects_malformed_input_at_the_boundary() {
    let caps = Capabilities::all();
    let ctx = context();

    // Wrong type: must fail before reaching the port.
    let err = caps
        .call(&ctx, "issues.get", json!({ "id": 42 }))
        .unwrap_err();
    assert!(matches!(err, Error::Invalid(_)), "got {err:?}");

    // Missing required field.
    assert!(caps.call(&ctx, "issues.get", json!({})).is_err());

    // Validation inside the handler, not just deserialization.
    let err = caps
        .call(&ctx, "issues.create", json!({ "title": "   " }))
        .unwrap_err();
    assert!(matches!(err, Error::Invalid(_)), "got {err:?}");
}

#[test]
fn unknown_capability_is_a_not_found() {
    let caps = Capabilities::all();
    let ctx = context();
    let err = caps.call(&ctx, "issues.destroy", json!({})).unwrap_err();
    assert!(matches!(err, Error::CapabilityNotFound(_)));
    assert_eq!(err.kind(), aios_types::ErrorKind::NotFound);
}

#[test]
fn a_port_error_surfaces_rather_than_panicking() {
    let caps = Capabilities::all();
    let ctx = context();
    // NoKnowledge always fails; the capability must propagate it.
    let err = caps.call(&ctx, "kb.list", json!({})).unwrap_err();
    assert!(matches!(err, Error::NoVault(_)), "got {err:?}");
}
