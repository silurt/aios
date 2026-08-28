//! Port traits.
//!
//! Each port is the *neutral* interface to a class of tool (plan §2). The
//! adapter behind it is replaceable; the trait is what the capability registry,
//! the CLI, the MCP server, and every client depend on.
//!
//! Ports are stateless with respect to *which* project they act on: the repo
//! path is an argument rather than constructor state. That keeps a single
//! adapter instance usable across every registered project, which matters once
//! the daemon serves concurrent runs against different repos.

use aios_core::Result;
use aios_types::{
    Commit, Issue, IssueQuery, NewIssue, Note, NoteHit, NoteRef, RepoStatus, Scope, WriteNote,
};
use std::path::Path;

/// Issue tracking. First implementation: beads.
pub trait IssueTracker: Send + Sync {
    /// Name of the backing tool, for diagnostics and error messages.
    fn backend(&self) -> &'static str;

    /// Whether this repo has a tracker at all. Checked before every operation
    /// so the failure is "this project has no tracker" rather than whatever the
    /// underlying tool prints when it cannot find its database.
    fn available(&self, repo: &Path) -> bool;

    fn list(&self, repo: &Path, query: &IssueQuery) -> Result<Vec<Issue>>;

    /// Issues with no unmet dependencies — work that can start now.
    ///
    /// This is the operation that justifies beads over a flat tracker, and it
    /// is why the port has it as a first-class method rather than leaving
    /// callers to reconstruct it from a dependency graph.
    fn ready(&self, repo: &Path) -> Result<Vec<Issue>>;

    fn get(&self, repo: &Path, id: &str) -> Result<Issue>;
    fn create(&self, repo: &Path, new: &NewIssue) -> Result<Issue>;
    fn close(&self, repo: &Path, id: &str, reason: Option<&str>) -> Result<Issue>;
}

/// Knowledge base. First implementation: an Obsidian vault.
pub trait Knowledge: Send + Sync {
    fn backend(&self) -> &'static str;
    fn available(&self) -> bool;

    fn list(&self, scope: &Scope) -> Result<Vec<NoteRef>>;
    fn search(&self, scope: &Scope, query: &str, limit: usize) -> Result<Vec<NoteHit>>;
    fn read(&self, path: &str) -> Result<Note>;
    fn write(&self, req: &WriteNote) -> Result<Note>;
}

/// A coding harness — Claude Code, Codex, or anything else that can be driven
/// headlessly.
///
/// Both current harnesses are driven the same way: a subprocess emitting
/// newline-delimited JSON. The Claude Agent SDK is TypeScript/Python-only and
/// therefore out of reach from Rust, which turns out to be a simplification —
/// one mechanism, one parser shape, no per-harness special case.
///
/// The trait deliberately does *not* own the process. It builds the command and
/// translates the output; the supervisor owns spawning, streaming and lifetime,
/// so that concern lives in one place regardless of which harness is running.
pub trait Harness: Send + Sync {
    fn id(&self) -> aios_types::HarnessId;

    /// The binary this harness needs on PATH.
    fn binary(&self) -> &'static str;

    /// Arguments for a fresh run in `cwd`.
    fn command(&self, spec: &RunSpec) -> Vec<String>;

    /// Arguments for resuming a previous session.
    ///
    /// `None` means the harness cannot resume, in which case a parked run has
    /// to be restarted rather than continued — worth knowing explicitly rather
    /// than discovering when it silently starts over.
    fn resume_command(&self, spec: &RunSpec, session_ref: &str) -> Option<Vec<String>>;

    /// Translate one line of the harness's output into normalized events.
    ///
    /// Returns a vector because one native event can carry several — an
    /// assistant message with two tool calls is three normalized events — and
    /// empty because most lines are noise no consumer should see.
    fn translate(&self, line: &str) -> Vec<aios_types::RunEvent>;
}

/// What to run, independent of which harness runs it.
#[derive(Debug, Clone)]
pub struct RunSpec {
    pub prompt: String,
    pub cwd: std::path::PathBuf,
    pub model: Option<String>,
    /// Tools pre-authorised by policy, so the harness never asks about them.
    /// This is where most approvals are avoided rather than answered (§7.1).
    pub allowed_tools: Vec<String>,
    pub disallowed_tools: Vec<String>,
}

/// Version control. First (and for now only) implementation: git.
pub trait Vcs: Send + Sync {
    fn backend(&self) -> &'static str;
    fn status(&self, repo: &Path) -> Result<RepoStatus>;
    fn log(&self, repo: &Path, limit: usize) -> Result<Vec<Commit>>;

    /// A unified diff, capped in size.
    ///
    /// The cap is part of the contract rather than a caller's problem: a
    /// whole-repo diff can be megabytes, and handing that to a UI or a model
    /// unannounced is worse than truncating and saying so.
    fn diff(
        &self,
        repo: &Path,
        base: Option<&str>,
        staged: bool,
        max_bytes: usize,
    ) -> Result<aios_types::Diff>;
}
