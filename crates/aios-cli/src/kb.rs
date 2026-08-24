//! `aios kb …` — the knowledge base.

use crate::render::{bold, dim, green};
use anyhow::Result;
use clap::Subcommand;
use serde_json::json;

#[derive(Subcommand)]
pub enum KbCommand {
    /// List notes
    #[command(visible_alias = "ls")]
    List {
        /// Scope to a project's notes
        #[arg(long, short)]
        project: Option<String>,
        /// Scope to shared, non-project knowledge
        #[arg(long, conflicts_with = "project")]
        global: bool,
        /// Scope to the capture inbox
        #[arg(long, conflicts_with_all = ["project", "global"])]
        inbox: bool,
    },
    /// Search notes
    Search {
        query: String,
        #[arg(long, short)]
        project: Option<String>,
        #[arg(long, conflicts_with = "project")]
        global: bool,
        #[arg(long, default_value_t = 25)]
        limit: usize,
    },
    /// Print a note
    Read { path: String },
    /// Create or overwrite a note
    Write {
        path: String,
        /// Body text. Reads stdin when omitted.
        body: Option<String>,
        #[arg(long, short)]
        title: Option<String>,
        #[arg(long = "tag")]
        tags: Vec<String>,
        /// Append instead of replacing
        #[arg(long, short)]
        append: bool,
    },
    /// Append a quick note to today's inbox entry
    Capture {
        /// Text to capture. Reads stdin when omitted.
        body: Option<String>,
        #[arg(long = "tag")]
        tags: Vec<String>,
    },
}

/// `--global` / `--inbox` / `--project` are mutually exclusive at the clap
/// level, so this only has to pick the one that was set.
fn scope(project: &Option<String>, global: bool, inbox: bool) -> serde_json::Value {
    if inbox {
        json!({ "scope": { "type": "inbox" } })
    } else if global {
        json!({ "scope": { "type": "global" } })
    } else if let Some(p) = project {
        json!({ "project": p })
    } else {
        json!({})
    }
}

fn body_or_stdin(body: Option<String>) -> Result<String> {
    match body {
        Some(b) => Ok(b),
        None => Ok(std::io::read_to_string(std::io::stdin())?),
    }
}

pub fn run(cmd: KbCommand, json_out: bool) -> Result<()> {
    let caps = crate::app::capabilities();
    let ctx = crate::app::context()?;

    let (name, input) = match cmd {
        KbCommand::List {
            project,
            global,
            inbox,
        } => ("kb.list", scope(&project, global, inbox)),
        KbCommand::Search {
            query,
            project,
            global,
            limit,
        } => {
            let mut v = scope(&project, global, false);
            v["query"] = json!(query);
            v["limit"] = json!(limit);
            ("kb.search", v)
        }
        KbCommand::Read { path } => ("kb.read", json!({ "path": path })),
        KbCommand::Write {
            path,
            body,
            title,
            tags,
            append,
        } => (
            "kb.write",
            json!({
                "path": path,
                "body": body_or_stdin(body)?,
                "title": title,
                "tags": tags,
                "append": append,
            }),
        ),
        KbCommand::Capture { body, tags } => (
            "kb.capture",
            json!({ "body": body_or_stdin(body)?, "tags": tags }),
        ),
    };

    let result = caps.call(&ctx, name, input)?;
    if json_out {
        println!("{}", serde_json::to_string_pretty(&result)?);
        return Ok(());
    }

    match name {
        "kb.list" => print_refs(&result),
        "kb.search" => print_hits(&result),
        "kb.read" => {
            println!("{}", bold(result["title"].as_str().unwrap_or("")));
            println!("{}", dim(result["path"].as_str().unwrap_or("")));
            if let Some(links) = result["links"].as_array().filter(|l| !l.is_empty()) {
                let names: Vec<_> = links.iter().filter_map(|l| l.as_str()).collect();
                println!("{} {}", dim("links:"), names.join(", "));
            }
            println!("\n{}", result["body"].as_str().unwrap_or(""));
        }
        _ => println!(
            "{} {}",
            green("wrote"),
            bold(result["path"].as_str().unwrap_or(""))
        ),
    }
    Ok(())
}

fn print_refs(value: &serde_json::Value) {
    let Some(items) = value.as_array() else {
        return;
    };
    if items.is_empty() {
        println!("{}", dim("no notes"));
        return;
    }
    for n in items {
        println!(
            "{}  {}",
            bold(n["title"].as_str().unwrap_or("")),
            dim(n["path"].as_str().unwrap_or(""))
        );
    }
}

fn print_hits(value: &serde_json::Value) {
    let Some(items) = value.as_array() else {
        return;
    };
    if items.is_empty() {
        println!("{}", dim("no matches"));
        return;
    }
    for h in items {
        println!(
            "{}{}",
            bold(h["path"].as_str().unwrap_or("")),
            dim(&format!(":{}", h["line"]))
        );
        println!("  {}", h["excerpt"].as_str().unwrap_or(""));
    }
}
