//! Knowledge-base types.
//!
//! Neutral over "a tree of markdown files with frontmatter and wikilinks",
//! which is what Obsidian is on disk. Nothing here mentions Obsidian, so a
//! different store slots in behind the same port.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Which slice of the vault an operation applies to.
///
/// Internally tagged per §15 so this generates a real Swift enum rather than
/// an awkward optional-soup struct.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema, JsonSchema)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Scope {
    /// Knowledge that is not tied to any one project.
    Global,
    /// A single project's notes, addressed by registry slug.
    Project { slug: String },
    /// The capture surface — quick notes that have not been filed yet.
    Inbox,
    /// Everything.
    #[default]
    All,
}

impl Scope {
    /// The vault subdirectory this scope maps to, or `None` for [`Scope::All`].
    pub fn subdir(&self) -> Option<String> {
        match self {
            Scope::Global => Some("global".into()),
            Scope::Project { slug } => Some(format!("projects/{slug}")),
            Scope::Inbox => Some("inbox".into()),
            Scope::All => None,
        }
    }
}

/// A note's identity and metadata, without its body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NoteRef {
    /// Vault-relative path including the `.md` extension. The stable id.
    pub path: String,
    /// Frontmatter `title`, falling back to the first heading, then the stem.
    pub title: String,
    #[serde(default)]
    pub tags: Vec<String>,
    pub modified_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    #[serde(flatten)]
    pub meta: NoteRef,
    /// Markdown body with frontmatter stripped.
    pub body: String,
    /// `[[wikilink]]` targets found in the body, resolved to vault paths where
    /// a matching note exists and left as bare names where it does not — an
    /// unresolved link is a real signal, not an error.
    #[serde(default)]
    pub links: Vec<String>,
}

/// A search result: the note plus why it matched.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NoteHit {
    #[serde(flatten)]
    pub meta: NoteRef,
    /// 1-indexed line number of the first match.
    pub line: u32,
    /// The matching line, trimmed.
    pub excerpt: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct WriteNote {
    /// Vault-relative path. `.md` is appended when missing.
    pub path: String,
    pub body: String,
    pub title: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    /// Append to an existing note instead of replacing it. Append is the safe
    /// default for agents: it cannot destroy what it did not read.
    #[serde(default)]
    pub append: bool,
}
