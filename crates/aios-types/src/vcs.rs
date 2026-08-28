//! Version-control types.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RepoStatus {
    pub branch: Option<String>,
    /// Files with staged or unstaged modifications, capped for display.
    #[serde(default)]
    pub changed_files: Vec<FileChange>,
    pub staged: u32,
    pub unstaged: u32,
    pub untracked: u32,
    /// Commits ahead of / behind the upstream, when one is configured.
    pub ahead: Option<u32>,
    pub behind: Option<u32>,
    pub clean: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FileChange {
    pub path: String,
    /// Porcelain status code, e.g. `M`, `A`, `??`.
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Commit {
    pub sha: String,
    pub subject: String,
    pub author: String,
    pub date: String,
}

/// A unified diff of the working tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Diff {
    /// What was compared, e.g. `working tree` or `HEAD~1..HEAD`.
    pub against: String,
    /// Unified diff text. Empty when there is nothing to show.
    pub patch: String,
    /// Files touched, for rendering a summary without parsing the patch.
    #[serde(default)]
    pub files: Vec<String>,
    /// True when the patch was cut short by `maxBytes`. A client must say so
    /// rather than presenting a truncated diff as the whole change.
    pub truncated: bool,
}
