use crate::context::Context;
use crate::registry::{Capability, Effect};
use aios_core::Result;
use aios_types::{Commit, RepoStatus};
use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct StatusInput {
    pub project: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LogInput {
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

pub fn register(items: &mut Vec<Capability>) {
    items.push(Capability::new(
        "vcs.status",
        "Working-tree status: branch, changed files, ahead/behind",
        Effect::Read,
        |ctx: &Context, input: StatusInput| -> Result<RepoStatus> {
            let repo = ctx.repo_path(input.project.as_deref())?;
            ctx.ports.vcs.status(&repo)
        },
    ));

    items.push(Capability::new(
        "vcs.log",
        "Recent commits",
        Effect::Read,
        |ctx: &Context, input: LogInput| -> Result<Vec<Commit>> {
            let repo = ctx.repo_path(input.project.as_deref())?;
            ctx.ports.vcs.log(&repo, input.limit)
        },
    ));
}
