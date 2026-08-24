//! The single definition of every type that crosses a boundary.
//!
//! See `docs/plan.md` §15. Nothing may reach the API, the MCP server, or a client
//! without being defined here: `#[utoipa::path]` will not compile unless a
//! handler's types implement [`utoipa::ToSchema`], which makes the derivation
//! chain a compile-time requirement rather than a convention.
//!
//! House rules, enforced by review and by the round-trip fixtures:
//!
//! - `#[serde(rename_all = "camelCase")]` on everything, so generated Swift and
//!   TypeScript are idiomatic with no per-client mapping layer.
//! - Enums are **internally tagged** (`#[serde(tag = "type")]`). Untagged and
//!   externally-tagged representations generate poor or wrong Swift.
//! - Newtype ids, never a bare `String`, so generated clients cannot transpose
//!   two identifiers.
//! - No `serde_json::Value` in a wire type. An untyped hole is a hole in the
//!   contract.

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use utoipa::ToSchema;

pub mod ids;
pub mod inputs;
pub mod issue;
pub mod knowledge;
pub mod project;
pub mod run;
pub mod vcs;

pub use ids::ProjectId;
pub use inputs::{
    CaptureNoteInput, CloseIssueInput, CreateIssueInput, GetIssueInput, ListIssuesInput,
    ListProjectsInput, LogInput, ProjectRef, ReadNoteInput, ScopedInput, SearchNotesInput,
};
pub use issue::{Issue, IssueQuery, IssueStatus, NewIssue};
pub use knowledge::{Note, NoteHit, NoteRef, Scope, WriteNote};
pub use project::{NewProject, Project, ProjectDetection, ProjectSummary};
pub use run::{
    Approval, ApprovalId, ApprovalState, Decider, HarnessId, MessageRole, Run, RunEvent, RunId,
    RunStatus,
};
pub use vcs::{Commit, FileChange, RepoStatus};

/// Wire-format timestamp. Always serialized as RFC 3339.
pub type Timestamp = OffsetDateTime;

/// The API contract version.
///
/// Monotonic, and bumped **only** when `openapi.json` changes (§15.3). It is
/// deliberately not the crate version: the repo version moves for reasons that
/// never touch the contract, and compatibility must be an integer comparison
/// rather than semver range logic.
pub const API_VERSION: u32 = 1;

/// The oldest client contract this build still serves. Raised only on a
/// breaking change.
pub const MIN_CLIENT_API: u32 = 1;

/// Version and build information, served at `/api/version` once the daemon
/// exists and printed by `aios --version` today.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct VersionInfo {
    /// Contract version. Compatibility decisions use this and nothing else.
    pub api_version: u32,
    /// Oldest client contract still served.
    pub min_client_api: u32,
    /// Repo semver. Diagnostics and display only.
    pub daemon_version: String,
}

impl VersionInfo {
    pub fn current() -> Self {
        Self {
            api_version: API_VERSION,
            min_client_api: MIN_CLIENT_API,
            daemon_version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }
}

/// Wire-level error. Every failing response is one of these, so clients can
/// branch on `kind` rather than parsing prose.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApiError {
    pub kind: ErrorKind,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum ErrorKind {
    NotFound,
    AlreadyExists,
    InvalidArgument,
    FailedPrecondition,
    Internal,
}
