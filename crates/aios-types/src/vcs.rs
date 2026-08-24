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
