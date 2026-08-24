//! `aios approval …` — the triage queue.

use crate::render::{bold, dim, green, red, yellow};
use anyhow::Result;
use clap::Subcommand;

#[derive(Subcommand)]
pub enum ApprovalCommand {
    /// Approvals waiting on you
    #[command(visible_alias = "ls")]
    List {
        /// Include ones already decided
        #[arg(long)]
        all: bool,
    },
    /// Show one approval in full
    Show { id: String },
    /// Approve a request
    Approve {
        id: String,
        #[arg(long, short)]
        reason: Option<String>,
    },
    /// Deny a request
    Deny {
        id: String,
        #[arg(long, short)]
        reason: Option<String>,
    },
    /// Print the effective policy
    Policy,

    /// Decide a harness permission request read from stdin
    ///
    /// Invoked by a PreToolUse hook, not by hand. Blocks while a decision is
    /// outstanding, which is what makes the harness wait at the gate instead of
    /// failing.
    Gate,
}

pub fn run(cmd: ApprovalCommand, json: bool) -> Result<()> {
    // The gate runs inside a harness process and must work whether or not the
    // daemon happens to be up, so it keeps its own direct path; everything a
    // human types goes through the API like any other client.
    if let ApprovalCommand::Gate = cmd {
        return crate::gate::run();
    }
    let client = crate::client::Client::connect()?;
    match cmd {
        ApprovalCommand::Gate => unreachable!("handled above"),
        ApprovalCommand::List { all } => {
            let listed = client.get("/api/approvals")?;
            let items: Vec<serde_json::Value> = listed
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter(|a| all || a["state"] == "pending")
                .collect();
            if json {
                println!("{}", serde_json::to_string_pretty(&items)?);
                return Ok(());
            }
            if items.is_empty() {
                println!("{}", dim("nothing waiting"));
                return Ok(());
            }
            for a in &items {
                println!(
                    "{} {} {} {}",
                    state_badge(a["state"].as_str().unwrap_or("")),
                    bold(a["id"].as_str().unwrap_or("")),
                    bold(a["tool"].as_str().unwrap_or("")),
                    dim(a["summary"].as_str().unwrap_or(""))
                );
            }
            Ok(())
        }
        ApprovalCommand::Show { id } => {
            let a = client.get(&format!("/api/approvals/{id}"))?;
            if json {
                println!("{}", serde_json::to_string_pretty(&a)?);
                return Ok(());
            }
            let field = |k: &str, v: &str| println!("{} {v}", dim(&format!("{k:<10}")));
            let s = |k: &str| a[k].as_str().unwrap_or("—").to_string();
            println!(
                "{} {}",
                bold(&s("id")),
                state_badge(a["state"].as_str().unwrap_or(""))
            );
            field("tool", &s("tool"));
            field("run", &s("runId"));
            field("project", &s("project"));
            field("rule", &s("rule"));
            field("expires", &s("expiresAt"));
            println!("\n{}", s("summary"));
            if let Some(detail) = a["detail"].as_str() {
                println!("\n{detail}");
            }
            Ok(())
        }
        ApprovalCommand::Approve { id, reason } => decide(&client, &id, true, reason, json),
        ApprovalCommand::Deny { id, reason } => decide(&client, &id, false, reason, json),
        ApprovalCommand::Policy => {
            let policy = crate::app::policy()?;
            if json {
                println!("{}", serde_json::to_string_pretty(&policy)?);
                return Ok(());
            }
            println!("{} {}s\n", dim("timeout"), policy.timeout_secs);
            for rule in &policy.rules {
                println!(
                    "{} {} {}",
                    verdict_badge(rule.verdict),
                    bold(&rule.name),
                    dim(&match &rule.contains {
                        Some(c) => format!("{} containing {c:?}", rule.tool),
                        None => rule.tool.clone(),
                    })
                );
            }
            println!("\n{} {}", dim("default"), verdict_badge(policy.default));
            Ok(())
        }
    }
}

fn decide(
    client: &crate::client::Client,
    id: &str,
    approve: bool,
    reason: Option<String>,
    json: bool,
) -> Result<()> {
    let a = client.post(
        &format!("/api/approvals/{id}/decide"),
        &serde_json::json!({ "approve": approve, "reason": reason }),
    )?;
    if json {
        println!("{}", serde_json::to_string_pretty(&a)?);
    } else {
        println!(
            "{} {} {}",
            if approve {
                green("approved")
            } else {
                red("denied")
            },
            bold(a["id"].as_str().unwrap_or("")),
            dim(a["summary"].as_str().unwrap_or(""))
        );
    }
    Ok(())
}

fn state_badge(state: &str) -> String {
    match state {
        "pending" => yellow("pending "),
        "approved" => green("approved"),
        "denied" => red("denied  "),
        "expired" => dim("expired "),
        other => dim(other),
    }
}

fn verdict_badge(v: aios_runs::Verdict) -> String {
    match v {
        aios_runs::Verdict::Allow => green("allow"),
        aios_runs::Verdict::Deny => red("deny "),
        aios_runs::Verdict::Ask => yellow("ask  "),
    }
}
