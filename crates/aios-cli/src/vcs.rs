//! `aios vcs …`

use crate::render::{bold, dim, green, yellow};
use anyhow::Result;
use clap::Subcommand;
use serde_json::json;

#[derive(Subcommand)]
pub enum VcsCommand {
    /// Working-tree status
    Status {
        #[arg(long, short)]
        project: Option<String>,
    },
    /// Recent commits
    Log {
        #[arg(long, short)]
        project: Option<String>,
        #[arg(long, short = 'n', default_value_t = 20)]
        limit: usize,
    },
}

pub fn run(cmd: VcsCommand, json_out: bool) -> Result<()> {
    let caps = crate::app::capabilities();
    let ctx = crate::app::context()?;
    let (name, input) = match &cmd {
        VcsCommand::Status { project } => ("vcs.status", json!({ "project": project })),
        VcsCommand::Log { project, limit } => {
            ("vcs.log", json!({ "project": project, "limit": limit }))
        }
    };
    let result = caps.call(&ctx, name, input)?;
    if json_out {
        println!("{}", serde_json::to_string_pretty(&result)?);
        return Ok(());
    }

    match cmd {
        VcsCommand::Status { .. } => {
            let branch = result["branch"].as_str().unwrap_or("(detached)");
            let mut line = bold(branch);
            if let Some(a) = result["ahead"].as_u64().filter(|n| *n > 0) {
                line.push_str(&format!(" {}", green(&format!("↑{a}"))));
            }
            if let Some(b) = result["behind"].as_u64().filter(|n| *n > 0) {
                line.push_str(&format!(" {}", yellow(&format!("↓{b}"))));
            }
            println!("{line}");
            if result["clean"].as_bool().unwrap_or(false) {
                println!("{}", dim("clean"));
                return Ok(());
            }
            println!(
                "{}",
                dim(&format!(
                    "{} staged  {} unstaged  {} untracked",
                    result["staged"], result["unstaged"], result["untracked"]
                ))
            );
            for f in result["changedFiles"].as_array().into_iter().flatten() {
                println!(
                    "  {} {}",
                    yellow(f["status"].as_str().unwrap_or("?")),
                    f["path"].as_str().unwrap_or("")
                );
            }
        }
        VcsCommand::Log { .. } => {
            for c in result.as_array().into_iter().flatten() {
                let sha = c["sha"].as_str().unwrap_or("");
                println!(
                    "{} {} {}",
                    yellow(&sha.chars().take(8).collect::<String>()),
                    c["subject"].as_str().unwrap_or(""),
                    dim(c["author"].as_str().unwrap_or("")),
                );
            }
        }
    }
    Ok(())
}
