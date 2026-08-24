//! Issue-tracker types.
//!
//! Deliberately neutral: these describe an issue, not a *beads* issue. The
//! beads adapter normalizes into this shape so that swapping in Linear or GitHub
//! later (plan §2) does not require rewriting prompts, skills, the CLI, or any
//! client.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Issue {
    /// Tracker-native id, e.g. `aios-r32`. Opaque; never parsed.
    pub id: String,
    pub title: String,
    pub status: IssueStatus,
    /// 0 is most urgent, matching beads. Normalized on the way in so callers
    /// never have to know a tracker's native scale.
    pub priority: u8,
    pub issue_type: String,
    pub description: Option<String>,
    #[serde(default)]
    pub labels: Vec<String>,
    pub assignee: Option<String>,
    /// Issues this one is blocked by.
    #[serde(default)]
    pub blocked_by: Vec<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

/// Normalized status. A tracker with richer states maps into the nearest of
/// these and keeps its native value in [`Issue::issue_type`]-adjacent metadata
/// rather than leaking a tracker-specific variant into the contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum IssueStatus {
    Open,
    InProgress,
    Blocked,
    Deferred,
    Closed,
}

impl IssueStatus {
    pub fn parse(s: &str) -> Self {
        match s {
            "in_progress" | "inProgress" | "hooked" => Self::InProgress,
            "blocked" => Self::Blocked,
            "deferred" | "pinned" => Self::Deferred,
            "closed" => Self::Closed,
            _ => Self::Open,
        }
    }

    /// The value to hand back to beads. `Deferred` maps to `deferred`; `pinned`
    /// is beads-specific and never round-trips.
    pub fn as_beads(&self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::InProgress => "in_progress",
            Self::Blocked => "blocked",
            Self::Deferred => "deferred",
            Self::Closed => "closed",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct IssueQuery {
    /// Restrict to these statuses. Empty means "whatever the tracker considers
    /// open", not "all" — closed issues are noise by default.
    #[serde(default)]
    pub status: Vec<IssueStatus>,
    pub search: Option<String>,
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NewIssue {
    pub title: String,
    pub description: Option<String>,
    /// `task`, `bug`, `feature`, `chore`, `epic`, … Defaults to `task`.
    pub issue_type: Option<String>,
    pub priority: Option<u8>,
    #[serde(default)]
    pub labels: Vec<String>,
}
