//! `aios cap …` — the capability registry, exposed directly.
//!
//! This is how §2 is demonstrated: the same registration that becomes an MCP
//! tool and a REST route is callable by name, JSON in and JSON out. Everything
//! here goes through the daemon, so it exercises exactly the path a client
//! takes rather than a shortcut only the CLI has (§3.1).

use crate::render::{bold, dim, yellow};
use anyhow::{Context as _, Result};
use clap::Subcommand;

#[derive(Subcommand)]
pub enum CapCommand {
    /// List every capability
    #[command(visible_alias = "ls")]
    List,
    /// Print the JSON Schema a capability accepts
    ///
    /// Exactly what an MCP client shows the model as `inputSchema`, so it is
    /// the fastest way to check what an agent will see.
    Schema { name: String },
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
    let client = crate::client::Client::connect()?;
    match cmd {
        CapCommand::List => {
            let listed = client.get("/api/capabilities")?;
            if json {
                println!("{}", serde_json::to_string_pretty(&listed)?);
                return Ok(());
            }
            let items = listed.as_array().cloned().unwrap_or_default();
            let width = items
                .iter()
                .filter_map(|c| c["name"].as_str().map(str::len))
                .max()
                .unwrap_or(0);
            for c in &items {
                let effect = if c["effect"] == "write" {
                    yellow("write")
                } else {
                    dim("read ")
                };
                println!(
                    "{effect} {}  {}",
                    bold(&format!(
                        "{:<width$}",
                        c["name"].as_str().unwrap_or(""),
                        width = width
                    )),
                    dim(c["summary"].as_str().unwrap_or(""))
                );
            }
            Ok(())
        }
        CapCommand::Schema { name } => {
            let listed = client.get("/api/capabilities")?;
            let found = listed
                .as_array()
                .and_then(|a| a.iter().find(|c| c["name"] == name.as_str()))
                .with_context(|| format!("no capability named {name:?}"))?;
            println!("{}", serde_json::to_string_pretty(&found["inputSchema"])?);
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
            // Output is always JSON: a capability's result is data for a
            // program, and `cap call` is the programmatic door.
            let result = client.call_capability(&name, value)?;
            println!("{}", serde_json::to_string_pretty(&result)?);
            Ok(())
        }
    }
}
