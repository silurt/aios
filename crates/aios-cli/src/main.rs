//! `aios` — one binary, three modes (plan §3.1).
//!
//! Phase 0 implements the client mode only: there is no daemon yet, so commands
//! call `aios-core` in process. When the daemon lands in phase 4 these handlers
//! become API calls over the Unix socket, which is why they are kept thin and
//! do no work beyond argument handling and rendering.

mod app;
mod approval;
mod cap;
mod gate;
mod issue;
mod kb;
mod mcp;
mod project;
mod render;
mod run;
mod vcs;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "aios",
    version,
    about = "An operating layer for running coding agents across your projects",
    long_about = None,
    propagate_version = true
)]
struct Cli {
    /// Emit JSON instead of formatted text. Set automatically when stdout is
    /// not a terminal, so piping into `jq` needs no flag.
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Register and inspect projects
    #[command(subcommand, visible_alias = "p")]
    Project(project::ProjectCommand),

    /// Issue tracking
    #[command(subcommand, visible_alias = "i")]
    Issue(issue::IssueCommand),

    /// Knowledge base
    #[command(subcommand)]
    Kb(kb::KbCommand),

    /// Version control
    #[command(subcommand)]
    Vcs(vcs::VcsCommand),

    /// Inspect and invoke capabilities directly
    #[command(subcommand)]
    Cap(cap::CapCommand),

    /// Serve capabilities to coding harnesses over MCP
    #[command(subcommand)]
    Mcp(mcp::McpCommand),

    /// Start and inspect harness runs
    #[command(subcommand, visible_alias = "r")]
    Run(run::RunCommand),

    /// Decide what agents are allowed to do
    #[command(subcommand, visible_alias = "a")]
    Approval(approval::ApprovalCommand),

    /// Check that the local installation is healthy
    Doctor,

    /// Print version and API contract information
    Version,
}

fn main() -> std::process::ExitCode {
    let cli = Cli::parse();
    let json = cli.json || !std::io::IsTerminal::is_terminal(&std::io::stdout());

    match run(cli.command, json) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{} {e}", render::red("error:"));
            for cause in e.chain().skip(1) {
                eprintln!("  {} {cause}", render::dim("caused by:"));
            }
            std::process::ExitCode::FAILURE
        }
    }
}

fn run(command: Command, json: bool) -> Result<()> {
    match command {
        Command::Project(cmd) => project::run(cmd, json),
        Command::Issue(cmd) => issue::run(cmd, json),
        Command::Kb(cmd) => kb::run(cmd, json),
        Command::Vcs(cmd) => vcs::run(cmd, json),
        Command::Cap(cmd) => cap::run(cmd, json),
        Command::Mcp(cmd) => mcp::run(cmd),
        Command::Run(cmd) => run::run(cmd, json),
        Command::Approval(cmd) => approval::run(cmd, json),
        Command::Doctor => doctor(json),
        Command::Version => version(json),
    }
}

fn version(json: bool) -> Result<()> {
    let info = aios_types::VersionInfo::current();
    if json {
        println!("{}", serde_json::to_string_pretty(&info)?);
    } else {
        println!("aios {}", info.daemon_version);
        println!(
            "api  {} (serving clients >= {})",
            info.api_version, info.min_client_api
        );
    }
    Ok(())
}

fn doctor(json: bool) -> Result<()> {
    use aios_core::config;

    let home = config::home();
    let cfg = config::Config::load()?;
    let registry = aios_core::Registry::open()?;
    let checks = vec![
        ("aios home", home.display().to_string(), home.exists()),
        (
            "projects",
            config::projects_dir().display().to_string(),
            config::projects_dir().exists(),
        ),
        ("vault", cfg.vault.display().to_string(), cfg.vault.exists()),
        (
            "beads (bd)",
            which("bd").unwrap_or_else(|| "not found".into()),
            which("bd").is_some(),
        ),
        (
            "git",
            which("git").unwrap_or_else(|| "not found".into()),
            which("git").is_some(),
        ),
        (
            "claude",
            which("claude").unwrap_or_else(|| "not found".into()),
            which("claude").is_some(),
        ),
        (
            "codex",
            which("codex").unwrap_or_else(|| "not found".into()),
            which("codex").is_some(),
        ),
    ];

    // These files are meant to be hand-edited, so checking them is part of a
    // health check rather than an afterthought.
    let problems = registry.validate();

    if json {
        let out: Vec<_> = checks
            .iter()
            .map(|(name, detail, ok)| {
                serde_json::json!({ "check": name, "detail": detail, "ok": ok })
            })
            .collect();
        let issues: Vec<_> = problems
            .iter()
            .map(|p| serde_json::json!({ "file": p.file, "detail": p.detail, "fix": p.fix }))
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "checks": out,
                "problems": issues,
                "projects": registry.count().unwrap_or(0),
            }))?
        );
        return Ok(());
    }

    // Flag, do not delete: removing someone's data behind their back is worse
    // than a line of output they can act on.
    if config::legacy_sqlite_path().exists() {
        println!(
            "{} unused {} left by an older build — safe to delete",
            render::yellow("note"),
            render::dim(&config::legacy_sqlite_path().display().to_string())
        );
    }

    for (name, detail, ok) in &checks {
        let mark = if *ok {
            render::green("ok  ")
        } else {
            render::yellow("miss")
        };
        println!("{mark} {name:<16} {}", render::dim(detail));
    }
    println!();
    if problems.is_empty() {
        println!("{} projects registered", registry.count()?);
    } else {
        println!(
            "{} {} problem(s) in ~/.aios:",
            render::yellow("!"),
            problems.len()
        );
        for p in &problems {
            println!("  {}", render::bold(&p.file));
            println!("    {}", p.detail);
            println!("    {} {}", render::dim("fix:"), p.fix);
        }
    }
    Ok(())
}

/// Minimal PATH lookup — enough for `doctor`, and not worth a dependency.
fn which(bin: &str) -> Option<String> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(bin))
        .find(|candidate| candidate.is_file())
        .map(|p| p.display().to_string())
}
