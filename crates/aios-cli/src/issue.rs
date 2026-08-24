//! `aios issue …` — the ergonomic front for the `issues.*` capabilities.
//!
//! These call through `Capabilities` rather than the port directly, so the CLI
//! exercises the same path MCP and REST will take. A bug reachable from an
//! agent is therefore reachable from here too.

use crate::render::{bold, dim, green, red, yellow};
use anyhow::Result;
use clap::Subcommand;
use serde_json::json;

#[derive(Subcommand)]
pub enum IssueCommand {
    /// List issues
    #[command(visible_alias = "ls")]
    List {
        #[arg(long, short)]
        project: Option<String>,
        /// Free-text search
        #[arg(long, short)]
        search: Option<String>,
        /// Include closed issues
        #[arg(long)]
        all: bool,
        #[arg(long, default_value_t = 50)]
        limit: u32,
    },
    /// Issues with no unmet dependencies — what can be started now
    Ready {
        #[arg(long, short)]
        project: Option<String>,
    },
    /// Show one issue
    Show {
        id: String,
        #[arg(long, short)]
        project: Option<String>,
    },
    /// File a new issue
    New {
        title: String,
        #[arg(long, short)]
        description: Option<String>,
        /// task, bug, feature, chore, epic, …
        #[arg(long, short = 't', default_value = "task")]
        issue_type: String,
        /// 0 is most urgent. No short flag: `-p` means project everywhere.
        #[arg(long, default_value_t = 2)]
        priority: u8,
        #[arg(long, short)]
        project: Option<String>,
    },
    /// Close an issue
    Close {
        id: String,
        #[arg(long, short)]
        reason: Option<String>,
        #[arg(long, short)]
        project: Option<String>,
    },
    /// Counts by status
    Status {
        #[arg(long, short)]
        project: Option<String>,
    },
}

pub fn run(cmd: IssueCommand, json: bool) -> Result<()> {
    let caps = crate::app::capabilities();
    let ctx = crate::app::context()?;

    let (name, input) = match &cmd {
        IssueCommand::List {
            project,
            search,
            all,
            limit,
        } => (
            "issues.list",
            json!({
                "project": project,
                "search": search,
                "limit": limit,
                // An empty status list means "whatever the tracker calls open";
                // --all widens it by naming every status explicitly.
                "status": if *all {
                    vec!["open", "inProgress", "blocked", "deferred", "closed"]
                } else {
                    vec![]
                },
            }),
        ),
        IssueCommand::Ready { project } => ("issues.ready", json!({ "project": project })),
        IssueCommand::Show { id, project } => {
            ("issues.get", json!({ "id": id, "project": project }))
        }
        IssueCommand::New {
            title,
            description,
            issue_type,
            priority,
            project,
        } => (
            "issues.create",
            json!({
                "title": title,
                "description": description,
                "issueType": issue_type,
                "priority": priority,
                "project": project,
            }),
        ),
        IssueCommand::Close {
            id,
            reason,
            project,
        } => (
            "issues.close",
            json!({ "id": id, "reason": reason, "project": project }),
        ),
        IssueCommand::Status { project } => ("issues.status", json!({ "project": project })),
    };

    let result = caps.call(&ctx, name, input)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&result)?);
        return Ok(());
    }

    match cmd {
        IssueCommand::Status { .. } => {
            let c = &result;
            println!(
                "{} open   {} in progress   {} blocked   {} ready",
                bold(&c["open"].to_string()),
                bold(&c["inProgress"].to_string()),
                bold(&c["blocked"].to_string()),
                green(&c["ready"].to_string()),
            );
        }
        IssueCommand::Show { .. } | IssueCommand::New { .. } | IssueCommand::Close { .. } => {
            print_issue(&result);
        }
        _ => print_issue_list(&result),
    }
    Ok(())
}

fn status_badge(status: &str) -> String {
    match status {
        "closed" => green("closed "),
        "inProgress" => yellow("wip    "),
        "blocked" => red("blocked"),
        "deferred" => dim("defer  "),
        _ => dim("open   "),
    }
}

fn print_issue_list(value: &serde_json::Value) {
    let Some(items) = value.as_array() else {
        return;
    };
    if items.is_empty() {
        println!("{}", dim("no issues"));
        return;
    }
    let width = items
        .iter()
        .filter_map(|i| i["id"].as_str().map(str::len))
        .max()
        .unwrap_or(0);
    for i in items {
        let id = i["id"].as_str().unwrap_or("?");
        println!(
            "{} {} {}",
            status_badge(i["status"].as_str().unwrap_or("open")),
            bold(&format!("{id:<width$}")),
            i["title"].as_str().unwrap_or(""),
        );
    }
}

fn print_issue(i: &serde_json::Value) {
    println!(
        "{} {}",
        bold(i["id"].as_str().unwrap_or("?")),
        i["title"].as_str().unwrap_or("")
    );
    let field = |k: &str, v: &str| println!("{} {v}", dim(&format!("{k:<10}")));
    field("status", i["status"].as_str().unwrap_or("?"));
    field("type", i["issueType"].as_str().unwrap_or("?"));
    field("priority", &i["priority"].to_string());
    if let Some(desc) = i["description"].as_str().filter(|d| !d.is_empty()) {
        println!("\n{desc}");
    }
}
