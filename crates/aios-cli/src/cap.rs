//! `aios cap …` — the capability registry, exposed directly.
//!
//! This is how phase 1 demonstrates §2: the same registration that will become
//! an MCP tool and a REST route is callable by name, with JSON in and JSON out,
//! today. If something works here it will work over every later surface, because
//! it is literally the same handler.

use crate::render::{bold, dim, yellow};
use anyhow::{Context as _, Result};
use clap::Subcommand;

#[derive(Subcommand)]
pub enum CapCommand {
    /// List every capability
    #[command(visible_alias = "ls")]
    List,
    /// Invoke a capability by name with a JSON payload
    Call {
        /// Capability name, e.g. `issues.ready`
        name: String,
        /// JSON input. Defaults to `{}`; reads stdin when given `-`.
        #[arg(default_value = "{}")]
        input: String,
    },
}

pub fn run(cmd: CapCommand, json: bool) -> Result<()> {
    let caps = crate::app::capabilities();
    match cmd {
        CapCommand::List => {
            if json {
                let out: Vec<_> = caps
                    .iter()
                    .map(|c| {
                        serde_json::json!({
                            "name": c.name,
                            "summary": c.summary,
                            "effect": if c.effect.is_write() { "write" } else { "read" },
                        })
                    })
                    .collect();
                println!("{}", serde_json::to_string_pretty(&out)?);
                return Ok(());
            }
            let width = caps.iter().map(|c| c.name.len()).max().unwrap_or(0);
            for c in caps.iter() {
                let effect = if c.effect.is_write() {
                    yellow("write")
                } else {
                    dim("read ")
                };
                println!(
                    "{effect} {}  {}",
                    bold(&format!("{:<width$}", c.name, width = width)),
                    dim(c.summary)
                );
            }
            Ok(())
        }
        CapCommand::Call { name, input } => {
            let raw = if input == "-" {
                std::io::read_to_string(std::io::stdin())?
            } else {
                input
            };
            let value: serde_json::Value = serde_json::from_str(raw.trim())
                .with_context(|| format!("input is not valid JSON: {raw}"))?;
            let ctx = crate::app::context()?;
            // Output is always JSON: a capability's result is data for a
            // program, and `cap call` is the programmatic door.
            let result = caps.call(&ctx, &name, value)?;
            println!("{}", serde_json::to_string_pretty(&result)?);
            Ok(())
        }
    }
}
