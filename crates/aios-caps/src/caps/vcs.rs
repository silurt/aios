use crate::context::Context;
use crate::registry::{Capability, Effect};
use aios_core::Result;
use aios_types::{Commit, LogInput, ProjectRef, RepoStatus};

pub fn register(items: &mut Vec<Capability>) {
    items.push(Capability::new(
        "vcs.status",
        "Working-tree status: branch, changed files, ahead/behind",
        Effect::Read,
        |ctx: &Context, input: ProjectRef| -> Result<RepoStatus> {
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
