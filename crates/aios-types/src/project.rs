//! The registry's view of a project.

use crate::{ProjectId, Timestamp};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// A project registered with AIOS.
///
/// `path` is the canonical on-disk location and is unique: registering the same
/// directory twice is an error rather than a duplicate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: ProjectId,
    /// Stable short name, unique across the registry. Used wherever a human or
    /// an agent names a project.
    pub slug: String,
    /// Display name. Defaults to the directory name.
    pub name: String,
    /// Absolute, canonicalized path to the working tree.
    pub path: String,
    /// `origin` remote URL, if the project has one.
    pub git_remote: Option<String>,
    /// Default branch as reported by git.
    pub default_branch: Option<String>,
    /// Detected languages, most significant first.
    pub languages: Vec<String>,
    /// Detected package manager, if exactly one was identified.
    pub package_manager: Option<String>,
    /// Beads issue prefix, when the project has a `.beads/` database.
    pub issue_prefix: Option<String>,
    /// Free-form tags for grouping and filtering.
    pub tags: Vec<String>,
    // The `Timestamp` alias hides `OffsetDateTime` from utoipa's derive, which
    // special-cases the literal type name. Stating the wire shape explicitly is
    // clearer for a contract crate regardless.
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub created_at: Timestamp,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub updated_at: Timestamp,
}

/// Reduced form for list views, so `project list` over a large registry does not
/// pay for fields nothing renders.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSummary {
    pub id: ProjectId,
    pub slug: String,
    pub name: String,
    pub path: String,
    pub languages: Vec<String>,
    pub tags: Vec<String>,
}

impl From<Project> for ProjectSummary {
    fn from(p: Project) -> Self {
        Self {
            id: p.id,
            slug: p.slug,
            name: p.name,
            path: p.path,
            languages: p.languages,
            tags: p.tags,
        }
    }
}

/// Request to register a project. Everything except `path` is optional and
/// filled in by detection when omitted.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NewProject {
    pub path: String,
    pub slug: Option<String>,
    pub name: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

/// What inspecting a directory told us about it. Kept separate from [`Project`]
/// so detection can be exercised — and shown by `project add --dry-run` —
/// without writing to the registry.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProjectDetection {
    pub git_remote: Option<String>,
    pub default_branch: Option<String>,
    pub languages: Vec<String>,
    pub package_manager: Option<String>,
    pub issue_prefix: Option<String>,
}
