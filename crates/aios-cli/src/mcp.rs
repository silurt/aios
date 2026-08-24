//! `aios mcp …` — serving the capability registry over MCP.

use crate::render::{bold, dim, green, tilde};
use anyhow::{Context, Result};
use clap::Subcommand;
use std::path::{Path, PathBuf};

#[derive(Subcommand)]
pub enum McpCommand {
    /// Serve over stdio. This is what a harness spawns; not useful by hand.
    Serve,
    /// Wire a project up so Claude Code and Codex both see AIOS
    Install {
        /// Project slug, id, or path. Defaults to the current directory.
        #[arg(default_value = ".")]
        project: String,
        /// Show what would be written without writing it
        #[arg(long)]
        dry_run: bool,
    },
    /// Remove AIOS wiring from a project
    Uninstall {
        #[arg(default_value = ".")]
        project: String,
    },
}

pub fn run(cmd: McpCommand) -> Result<()> {
    match cmd {
        McpCommand::Serve => serve(),
        McpCommand::Install { project, dry_run } => install(&project, dry_run),
        McpCommand::Uninstall { project } => uninstall(&project),
    }
}

/// The guidance block written into CLAUDE.md and AGENTS.md.
///
/// Deliberately short. It is prepended to every conversation in the project, so
/// it should orient rather than instruct at length — the tool schemas carry the
/// detail, and duplicating them here would just go stale.
const GUIDANCE: &str = r#"## AIOS

This project is registered with AIOS, which serves its tools over MCP as
`aios_*`. Prefer them over ad-hoc shell commands for these tasks:

- **Issues** — `aios_issues_ready` (unblocked work), `aios_issues_list`,
  `aios_issues_create`, `aios_issues_close`. Do not call `bd` directly.
- **Knowledge** — `aios_kb_search` before assuming something is undocumented;
  `aios_kb_capture` to record a decision worth keeping.
- **Projects** — `aios_projects_list` to see the other registered repos.

Every tool takes an optional `project` argument (slug, id, or path) and defaults
to the working directory, so it is normally omitted."#;

/// Resolve the aios binary to spawn.
///
/// The current executable, not a bare `aios` on PATH: a harness may run with a
/// different PATH than the shell that installed this, and silently binding to a
/// different build is worse than an absolute path that has to be reinstalled
/// after `cargo install`.
fn binary_path() -> Result<PathBuf> {
    std::env::current_exe().context("could not determine the running aios binary")
}

fn install(project: &str, dry_run: bool) -> Result<()> {
    let registry = aios_core::Registry::open()?;
    let project = registry.resolve(project)?;
    let root = PathBuf::from(&project.path);
    let binary = binary_path()?;

    let mcp_json = serde_json::json!({
        "mcpServers": {
            "aios": {
                "command": binary.display().to_string(),
                "args": ["mcp", "serve"],
            }
        }
    });

    let mut planned: Vec<(PathBuf, String)> = Vec::new();

    // Claude Code reads .mcp.json. Merge rather than overwrite: a project may
    // already have other servers configured, and clobbering them would be a
    // hostile thing for an install command to do.
    let mcp_path = root.join(".mcp.json");
    let merged = merge_mcp_json(&mcp_path, &mcp_json)?;
    if let Some(text) = merged {
        planned.push((mcp_path, text));
    }

    for name in ["CLAUDE.md", "AGENTS.md"] {
        let path = root.join(name);
        let existing = std::fs::read_to_string(&path).unwrap_or_default();
        match aios_core::managed::upsert(&existing, GUIDANCE) {
            Some(updated) => planned.push((path, updated)),
            None if existing.contains(aios_core::managed::BEGIN) => {}
            None => {
                eprintln!(
                    "{} {} has an unterminated AIOS block; left untouched",
                    crate::render::yellow("warning:"),
                    tilde(&path.display().to_string())
                );
            }
        }
    }

    if planned.is_empty() {
        println!("{}", dim("already up to date"));
        return Ok(());
    }

    for (path, contents) in planned {
        let shown = tilde(&path.display().to_string());
        if dry_run {
            println!("{} {}", bold("would write"), shown);
        } else {
            std::fs::write(&path, contents)
                .with_context(|| format!("writing {}", path.display()))?;
            println!("{} {}", green("wrote"), shown);
        }
    }
    if !dry_run {
        println!(
            "{}",
            dim("restart Claude Code or Codex in this project to pick it up")
        );
    }
    Ok(())
}

/// Merge our server entry into any existing `.mcp.json`, returning `None` when
/// the file already says what we would write.
fn merge_mcp_json(path: &Path, ours: &serde_json::Value) -> Result<Option<String>> {
    let mut root: serde_json::Value = match std::fs::read_to_string(path) {
        Ok(text) if !text.trim().is_empty() => serde_json::from_str(&text)
            .with_context(|| format!("{} is not valid JSON", path.display()))?,
        _ => serde_json::json!({}),
    };

    let entry = ours["mcpServers"]["aios"].clone();
    let servers = root
        .as_object_mut()
        .context("mcp config root must be an object")?
        .entry("mcpServers")
        .or_insert_with(|| serde_json::json!({}));
    let servers = servers
        .as_object_mut()
        .context("`mcpServers` must be an object")?;

    if servers.get("aios") == Some(&entry) {
        return Ok(None);
    }
    servers.insert("aios".into(), entry);
    let rendered = serde_json::to_string_pretty(&root)?;
    Ok(Some(format!("{rendered}\n")))
}

fn uninstall(project: &str) -> Result<()> {
    let registry = aios_core::Registry::open()?;
    let project = registry.resolve(project)?;
    let root = PathBuf::from(&project.path);

    let mcp_path = root.join(".mcp.json");
    // Collapsed into one let-chain: clippy is right that the nesting adds
    // nothing, and every condition here is "the file exists, parses, and has
    // our entry" — a single guard reads as exactly that.
    if let Ok(text) = std::fs::read_to_string(&mcp_path)
        && let Ok(mut config) = serde_json::from_str::<serde_json::Value>(&text)
        && let Some(servers) = config["mcpServers"].as_object_mut()
        && servers.remove("aios").is_some()
    {
        let rendered = serde_json::to_string_pretty(&config)?;
        std::fs::write(&mcp_path, format!("{rendered}\n"))?;
        println!(
            "{} {}",
            green("updated"),
            tilde(&mcp_path.display().to_string())
        );
    }

    for name in ["CLAUDE.md", "AGENTS.md"] {
        let path = root.join(name);
        let Ok(existing) = std::fs::read_to_string(&path) else {
            continue;
        };
        if let Some(cleaned) = aios_core::managed::remove(&existing) {
            std::fs::write(&path, cleaned)?;
            println!(
                "{} {}",
                green("updated"),
                tilde(&path.display().to_string())
            );
        }
    }
    Ok(())
}

fn serve() -> Result<()> {
    // stdout *is* the transport — a stray println here corrupts the protocol
    // stream, which surfaces as an opaque parse failure in the client rather
    // than as anything traceable. Everything diagnostic goes to stderr.
    let context = crate::app::context()?;
    let server = aios_mcp::AiosMcp::new(context);

    // A current-thread runtime: this process exists to shuttle one stdio
    // conversation, and capability work is handed to `spawn_blocking` anyway,
    // so worker threads would idle.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    eprintln!(
        "aios mcp: serving {} capabilities over stdio",
        server.tool_count()
    );
    runtime
        .block_on(server.serve_stdio())
        .map_err(|e| anyhow::anyhow!("mcp server failed: {e}"))
}
