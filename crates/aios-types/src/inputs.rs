//! Capability input types.
//!
//! These live here, not beside their handlers, because they cross boundaries:
//! each one is an MCP tool's `inputSchema`, a REST request body, and a CLI
//! `--input` payload. §15 says every such type has exactly one definition, and
//! this is it.
//!
//! Two conventions hold throughout:
//!
//! - `project: Option<String>` is a slug, id, or path, defaulting to the
//!   current directory.
//! - Fields are flat rather than `#[serde(flatten)]`ed. Flattening produces
//!   `allOf` in the generated JSON Schema, which reads poorly to a model
//!   choosing arguments and is handled inconsistently by MCP clients. An
//!   explicit field list is worth the small duplication.

use crate::{IssueStatus, Scope};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Common shape for capabilities that act on one project and take nothing else.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ProjectRef {
    /// Project slug, id, or path. Defaults to the current directory.
    pub project: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ListIssuesInput {
    /// Project slug, id, or path. Defaults to the current directory.
    pub project: Option<String>,
    /// Restrict to these statuses. Empty means whatever the tracker treats as
    /// open — closed issues are noise unless asked for.
    pub status: Vec<IssueStatus>,
    /// Free-text search across titles and descriptions.
    pub search: Option<String>,
    pub limit: Option<u32>,
}

impl Default for ListIssuesInput {
    fn default() -> Self {
        Self {
            project: None,
            status: Vec::new(),
            search: None,
            limit: Some(50),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GetIssueInput {
    /// Tracker-native id, e.g. `aios-r32`.
    pub id: String,
    #[serde(default)]
    /// Project slug, id, or path. Defaults to the current directory.
    pub project: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateIssueInput {
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    /// `task`, `bug`, `feature`, `chore`, `epic`, `decision`, `spike`, `story`.
    /// Defaults to `task`.
    #[serde(default)]
    pub issue_type: Option<String>,
    /// 0 is most urgent, 2 is the default.
    #[serde(default)]
    pub priority: Option<u8>,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    /// Project slug, id, or path. Defaults to the current directory.
    pub project: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CloseIssueInput {
    pub id: String,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    /// Project slug, id, or path. Defaults to the current directory.
    pub project: Option<String>,
}

/// `scope` and `project` are alternatives, never both — see
/// [`ScopedInput`] and the `kb.*` handlers.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ScopedInput {
    pub scope: Option<Scope>,
    /// Shorthand for a project scope — easier for a model to supply than a
    /// tagged union.
    pub project: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SearchNotesInput {
    pub query: String,
    #[serde(default)]
    pub scope: Option<Scope>,
    #[serde(default)]
    /// Project slug, id, or path. Defaults to the current directory.
    pub project: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReadNoteInput {
    /// Vault-relative path, e.g. `projects/aios/decisions.md`.
    pub path: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CaptureNoteInput {
    pub body: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct LogInput {
    /// Project slug, id, or path. Defaults to the current directory.
    pub project: Option<String>,
    pub limit: usize,
}

impl Default for LogInput {
    fn default() -> Self {
        Self {
            project: None,
            limit: 20,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ListProjectsInput {
    /// Only projects carrying this tag.
    pub tag: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct DiffInput {
    /// Project slug, id, or path. Defaults to the current directory.
    pub project: Option<String>,
    /// Include staged changes as well as unstaged. Default true.
    pub staged: bool,
    /// Compare against this ref instead of the working tree, e.g. `HEAD~1`.
    pub base: Option<String>,
    /// Cap on returned patch size. A whole-repo diff can be megabytes, which
    /// no UI wants and no model should be handed by accident.
    pub max_bytes: usize,
}

impl Default for DiffInput {
    fn default() -> Self {
        Self {
            project: None,
            staged: true,
            base: None,
            max_bytes: 200_000,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RunRef {
    /// Run id.
    pub run: String,
}
