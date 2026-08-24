//! `IssueTracker` backed by beads.
//!
//! Everything goes through the `bd` CLI. Reading `.beads/` directly is
//! forbidden (plan §6.1): the on-disk layout is beads' business, it is a Dolt
//! database rather than files we can parse, and an early attempt to shortcut
//! this by reading `config.yaml` silently returned the wrong answer.

use aios_caps::ports::IssueTracker;
use aios_core::{Error, Result};
use aios_types::{Issue, IssueQuery, IssueStatus, NewIssue};
use serde::Deserialize;
use std::path::Path;
use std::process::Command;

pub struct Beads;

impl Beads {
    pub fn new() -> Self {
        Self
    }

    fn run(&self, repo: &Path, args: &[&str]) -> Result<String> {
        let output = Command::new("bd")
            .arg("-C")
            .arg(repo)
            .args(args)
            .output()
            .map_err(|e| match e.kind() {
                std::io::ErrorKind::NotFound => Error::ToolMissing { tool: "bd".into() },
                _ => Error::Io(e),
            })?;

        if !output.status.success() {
            // bd reports failures on stderr, but falls back to stdout for some
            // commands; prefer whichever actually has content.
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let message = if stderr.is_empty() { stdout } else { stderr };
            return Err(Error::ToolFailed {
                tool: "bd".into(),
                message,
            });
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    fn query_issues(&self, repo: &Path, args: &[&str]) -> Result<Vec<Issue>> {
        let raw = self.run(repo, args)?;
        let parsed: Vec<BdIssue> =
            serde_json::from_str(raw.trim()).map_err(|e| Error::ToolFailed {
                tool: "bd".into(),
                message: format!("could not parse output: {e}"),
            })?;
        Ok(parsed.into_iter().map(Issue::from).collect())
    }
}

impl Default for Beads {
    fn default() -> Self {
        Self::new()
    }
}

impl IssueTracker for Beads {
    fn backend(&self) -> &'static str {
        "beads"
    }

    fn available(&self, repo: &Path) -> bool {
        repo.join(".beads").is_dir()
    }

    fn list(&self, repo: &Path, query: &IssueQuery) -> Result<Vec<Issue>> {
        // `bd search` and `bd list` are separate commands rather than one with a
        // flag, so the shape of the call depends on whether text was supplied.
        let limit = query.limit.unwrap_or(50).to_string();
        let mut issues = match &query.search {
            Some(text) if !text.trim().is_empty() => {
                self.query_issues(repo, &["search", text, "--json", "--limit", &limit])?
            }
            _ => self.query_issues(repo, &["list", "--json", "--limit", &limit])?,
        };
        if !query.status.is_empty() {
            issues.retain(|i| query.status.contains(&i.status));
        }
        Ok(issues)
    }

    fn ready(&self, repo: &Path) -> Result<Vec<Issue>> {
        self.query_issues(repo, &["ready", "--json"])
    }

    fn get(&self, repo: &Path, id: &str) -> Result<Issue> {
        // `bd show --json` returns an array even for a single id.
        self.query_issues(repo, &["show", id, "--json"])?
            .into_iter()
            .next()
            .ok_or_else(|| Error::NotFound {
                kind: "issue",
                id: id.to_string(),
            })
    }

    fn create(&self, repo: &Path, new: &NewIssue) -> Result<Issue> {
        let priority = new.priority.unwrap_or(2).to_string();
        let issue_type = new.issue_type.clone().unwrap_or_else(|| "task".into());
        let mut args: Vec<&str> = vec![
            "create",
            &new.title,
            "--json",
            "-p",
            &priority,
            "-t",
            &issue_type,
        ];
        if let Some(desc) = &new.description {
            args.extend_from_slice(&["-d", desc]);
        }
        for label in &new.labels {
            args.extend_from_slice(&["-l", label]);
        }

        // Unlike the query commands, `bd create --json` emits a single object.
        let raw = self.run(repo, &args)?;
        let parsed: BdIssue = serde_json::from_str(raw.trim()).map_err(|e| Error::ToolFailed {
            tool: "bd".into(),
            message: format!("could not parse created issue: {e}"),
        })?;
        Ok(parsed.into())
    }

    fn close(&self, repo: &Path, id: &str, reason: Option<&str>) -> Result<Issue> {
        let mut args: Vec<&str> = vec!["close", id];
        if let Some(reason) = reason {
            args.extend_from_slice(&["--reason", reason]);
        }
        self.run(repo, &args)?;
        // `bd close` does not return the issue, so read it back rather than
        // synthesizing a result that might not match what was stored.
        self.get(repo, id)
    }
}

/// beads' wire shape. Snake_case, and several fields are absent depending on
/// which command produced it — hence the pervasive `Option` and `default`.
#[derive(Debug, Deserialize)]
struct BdIssue {
    id: String,
    title: String,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    priority: Option<u8>,
    #[serde(default)]
    issue_type: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    labels: Vec<String>,
    #[serde(default)]
    assignee: Option<String>,
    #[serde(default)]
    blocked_by: Vec<String>,
    #[serde(default)]
    created_at: Option<String>,
    #[serde(default)]
    updated_at: Option<String>,
}

impl From<BdIssue> for Issue {
    fn from(b: BdIssue) -> Self {
        Issue {
            id: b.id,
            title: b.title,
            status: IssueStatus::parse(b.status.as_deref().unwrap_or("open")),
            priority: b.priority.unwrap_or(2),
            issue_type: b.issue_type.unwrap_or_else(|| "task".into()),
            description: b.description,
            labels: b.labels,
            assignee: b.assignee,
            blocked_by: b.blocked_by,
            created_at: b.created_at,
            updated_at: b.updated_at,
        }
    }
}
