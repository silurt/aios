use crate::context::Context;
use crate::registry::{Capability, Effect};
use aios_core::{Error, Result};
use aios_types::{
    CloseIssueInput, CreateIssueInput, GetIssueInput, Issue, IssueQuery, IssueStatus,
    ListIssuesInput, NewIssue, ProjectRef,
};
use schemars::JsonSchema;
use serde::Serialize;
use std::path::PathBuf;

/// Returned by `issues.status` — a summary cheap enough to render in a menu bar
/// or a widget without pulling every issue.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct IssueCounts {
    pub open: usize,
    pub in_progress: usize,
    pub blocked: usize,
    pub ready: usize,
}

/// Resolve the repo and confirm it actually has a tracker, so callers get
/// "this project has no issue tracker" rather than the backend's own message.
fn tracker_repo(ctx: &Context, project: Option<&str>) -> Result<PathBuf> {
    let repo = ctx.repo_path(project)?;
    if !ctx.ports.issues.available(&repo) {
        return Err(Error::NoIssueTracker(repo.display().to_string()));
    }
    Ok(repo)
}

pub fn register(items: &mut Vec<Capability>) {
    items.push(Capability::new(
        "issues.list",
        "List issues in a project, optionally filtered by status or text",
        Effect::Read,
        |ctx: &Context, input: ListIssuesInput| -> Result<Vec<Issue>> {
            let repo = tracker_repo(ctx, input.project.as_deref())?;
            ctx.ports.issues.list(
                &repo,
                &IssueQuery {
                    status: input.status,
                    search: input.search,
                    limit: input.limit,
                },
            )
        },
    ));

    items.push(Capability::new(
        "issues.ready",
        "Issues with no unmet dependencies — work that can start right now",
        Effect::Read,
        |ctx: &Context, input: ProjectRef| -> Result<Vec<Issue>> {
            let repo = tracker_repo(ctx, input.project.as_deref())?;
            ctx.ports.issues.ready(&repo)
        },
    ));

    items.push(Capability::new(
        "issues.get",
        "Fetch a single issue by id",
        Effect::Read,
        |ctx: &Context, input: GetIssueInput| -> Result<Issue> {
            let repo = tracker_repo(ctx, input.project.as_deref())?;
            ctx.ports.issues.get(&repo, &input.id)
        },
    ));

    items.push(Capability::new(
        "issues.create",
        "File a new issue",
        Effect::Write,
        |ctx: &Context, input: CreateIssueInput| -> Result<Issue> {
            let repo = tracker_repo(ctx, input.project.as_deref())?;
            if input.title.trim().is_empty() {
                return Err(Error::Invalid("title must not be empty".into()));
            }
            ctx.ports.issues.create(
                &repo,
                &NewIssue {
                    title: input.title,
                    description: input.description,
                    issue_type: input.issue_type,
                    priority: input.priority,
                    labels: input.labels,
                },
            )
        },
    ));

    items.push(Capability::new(
        "issues.close",
        "Close an issue, optionally with a reason",
        Effect::Write,
        |ctx: &Context, input: CloseIssueInput| -> Result<Issue> {
            let repo = tracker_repo(ctx, input.project.as_deref())?;
            ctx.ports
                .issues
                .close(&repo, &input.id, input.reason.as_deref())
        },
    ));

    items.push(Capability::new(
        "issues.status",
        "Counts of open, in-progress, blocked and ready issues",
        Effect::Read,
        |ctx: &Context, input: ProjectRef| -> Result<IssueCounts> {
            let repo = tracker_repo(ctx, input.project.as_deref())?;
            let all = ctx.ports.issues.list(&repo, &IssueQuery::default())?;
            let count = |s: IssueStatus| all.iter().filter(|i| i.status == s).count();
            Ok(IssueCounts {
                open: count(IssueStatus::Open),
                in_progress: count(IssueStatus::InProgress),
                blocked: count(IssueStatus::Blocked),
                ready: ctx.ports.issues.ready(&repo)?.len(),
            })
        },
    ));
}
