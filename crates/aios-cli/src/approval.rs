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
    let store = aios_runs::Approvals::open()?;
    match cmd {
        ApprovalCommand::List { all } => {
            let items = if all { store.all()? } else { store.pending()? };
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
                    state_badge(a.state),
                    bold(a.id.as_str()),
                    bold(&a.tool),
                    dim(&a.summary)
                );
            }
            Ok(())
        }
        ApprovalCommand::Show { id } => {
            let a = store.get(&id)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&a)?);
                return Ok(());
            }
            let field = |k: &str, v: &str| println!("{} {v}", dim(&format!("{k:<10}")));
            println!("{} {}", bold(a.id.as_str()), state_badge(a.state));
            field("tool", &a.tool);
            field("run", a.run_id.as_str());
            field("project", a.project.as_deref().unwrap_or("—"));
            field("rule", a.rule.as_deref().unwrap_or("—"));
            field("expires", &a.expires_at.to_string());
            println!("\n{}", a.summary);
            if let Some(detail) = &a.detail {
                println!("\n{detail}");
            }
            Ok(())
        }
        ApprovalCommand::Approve { id, reason } => decide(&store, &id, true, reason, json),
        ApprovalCommand::Deny { id, reason } => decide(&store, &id, false, reason, json),
        ApprovalCommand::Gate => crate::gate::run(),
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
    store: &aios_runs::Approvals,
    id: &str,
    approve: bool,
    reason: Option<String>,
    json: bool,
) -> Result<()> {
    let a = store.decide(id, approve, reason.as_deref())?;
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
            bold(a.id.as_str()),
            dim(&a.summary)
        );
    }
    Ok(())
}

fn state_badge(state: aios_types::ApprovalState) -> String {
    use aios_types::ApprovalState as S;
    match state {
        S::Pending => yellow("pending "),
        S::Approved => green("approved"),
        S::Denied => red("denied  "),
        S::Expired => dim("expired "),
    }
}

fn verdict_badge(v: aios_runs::Verdict) -> String {
    match v {
        aios_runs::Verdict::Allow => green("allow"),
        aios_runs::Verdict::Deny => red("deny "),
        aios_runs::Verdict::Ask => yellow("ask  "),
    }
}
