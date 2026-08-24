//! `aios serve` and `aios daemon …` — the always-on layer.
//!
//! One binary, three modes (plan §3.1). `serve` *is* the daemon; the `daemon`
//! subcommands manage a LaunchAgent that runs it. These are the only commands
//! permitted to touch `aios-core` directly — everything else is an API client.

use crate::render::{bold, dim, green, tilde, yellow};
use anyhow::{Context, Result};
use clap::Subcommand;
use std::path::PathBuf;

/// The launchd job label. Reverse-DNS by convention, and it is what every
/// `launchctl` call keys off, so it must not drift.
const LABEL: &str = "cc.rothert.aios";

#[derive(Subcommand)]
pub enum DaemonCommand {
    /// Install and start the LaunchAgent
    Install {
        /// Write the plist without loading it
        #[arg(long)]
        dry_run: bool,
    },
    /// Stop and remove the LaunchAgent
    Uninstall,
    Start,
    Stop,
    /// Whether the daemon is installed, loaded, and answering
    Status,
    /// Tail the daemon's log
    Logs {
        #[arg(long, short, default_value_t = 40)]
        lines: usize,
    },
}

pub fn socket_path() -> PathBuf {
    aios_core::config::home().join("aiosd.sock")
}

fn plist_path() -> PathBuf {
    dirs_home()
        .join("Library/LaunchAgents")
        .join(format!("{LABEL}.plist"))
}

fn log_path() -> PathBuf {
    aios_core::config::home().join("logs").join("aiosd.log")
}

fn dirs_home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default()
}

/// Run the daemon in the foreground. This is what launchd execs.
pub fn serve() -> Result<()> {
    let ports = crate::app::ports()?;
    let policy = crate::app::policy()?;
    let state = std::sync::Arc::new(
        aios_api::AppState::new(ports, policy).context("assembling daemon state")?,
    );

    let socket = socket_path();
    // stderr, not stdout: a daemon's stdout is captured by launchd into a log
    // file and nothing parses it, but keeping the two separate means `serve`
    // stays usable in a pipeline later.
    eprintln!("aios serve: listening on {}", socket.display());

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime
        .block_on(aios_api::serve_uds(state, &socket))
        .map_err(|e| anyhow::anyhow!("serve failed: {e}"))
}

pub fn run(cmd: DaemonCommand) -> Result<()> {
    match cmd {
        DaemonCommand::Install { dry_run } => install(dry_run),
        DaemonCommand::Uninstall => uninstall(),
        DaemonCommand::Start => launchctl(&["kickstart", &format!("gui/{}/{LABEL}", uid())]),
        DaemonCommand::Stop => launchctl(&["kill", "SIGTERM", &format!("gui/{}/{LABEL}", uid())]),
        DaemonCommand::Status => status(),
        DaemonCommand::Logs { lines } => logs(lines),
    }
}

fn uid() -> String {
    // launchd addresses per-user domains as gui/<uid>. `id -u` avoids a libc
    // dependency for one number.
    std::process::Command::new("id")
        .arg("-u")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

fn install(dry_run: bool) -> Result<()> {
    let binary = std::env::current_exe()?.canonicalize()?;
    let plist = plist_path();
    let log = log_path();
    std::fs::create_dir_all(log.parent().unwrap())?;

    // KeepAlive with SuccessfulExit=false restarts a crash but respects a
    // deliberate stop, which is what makes `daemon stop` mean stop rather than
    // "pause for ten seconds".
    let contents = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>{LABEL}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{}</string>
    <string>serve</string>
  </array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key>
  <dict><key>SuccessfulExit</key><false/></dict>
  <key>StandardOutPath</key><string>{}</string>
  <key>StandardErrorPath</key><string>{}</string>
  <key>ProcessType</key><string>Background</string>
</dict>
</plist>
"#,
        binary.display(),
        log.display(),
        log.display()
    );

    if dry_run {
        println!(
            "{} {}",
            bold("would write"),
            tilde(&plist.display().to_string())
        );
        println!("{contents}");
        return Ok(());
    }

    std::fs::create_dir_all(plist.parent().unwrap())?;
    std::fs::write(&plist, contents)?;
    println!("{} {}", green("wrote"), tilde(&plist.display().to_string()));

    // bootout first so reinstalling picks up a changed binary path instead of
    // silently keeping the old job.
    let domain = format!("gui/{}", uid());
    let _ = std::process::Command::new("launchctl")
        .args(["bootout", &format!("{domain}/{LABEL}")])
        .output();
    launchctl(&["bootstrap", &domain, &plist.display().to_string()])?;
    println!("{} {}", green("started"), dim(LABEL));
    Ok(())
}

fn uninstall() -> Result<()> {
    let domain = format!("gui/{}", uid());
    let _ = std::process::Command::new("launchctl")
        .args(["bootout", &format!("{domain}/{LABEL}")])
        .output();
    let plist = plist_path();
    if plist.exists() {
        std::fs::remove_file(&plist)?;
        println!(
            "{} {}",
            green("removed"),
            tilde(&plist.display().to_string())
        );
    } else {
        println!("{}", dim("not installed"));
    }
    Ok(())
}

fn launchctl(args: &[&str]) -> Result<()> {
    let output = std::process::Command::new("launchctl")
        .args(args)
        .output()
        .context("launchctl is unavailable — this is macOS-only")?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
        anyhow::bail!("launchctl {}: {}", args.join(" "), detail);
    }
    Ok(())
}

fn status() -> Result<()> {
    let plist = plist_path();
    let socket = socket_path();

    let loaded = std::process::Command::new("launchctl")
        .args(["print", &format!("gui/{}/{LABEL}", uid())])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    // Answering is the check that matters: the plist can exist and the job can
    // be loaded while the process is wedged.
    let answering = crate::client::Client::connect_existing().is_ok_and(|c| c.health().is_ok());

    let mark = |ok: bool| if ok { green("ok  ") } else { yellow("no  ") };
    println!(
        "{} plist    {}",
        mark(plist.exists()),
        dim(&tilde(&plist.display().to_string()))
    );
    println!("{} loaded   {}", mark(loaded), dim(LABEL));
    println!(
        "{} socket   {}",
        mark(socket.exists()),
        dim(&tilde(&socket.display().to_string()))
    );
    println!("{} answering", mark(answering));
    if !answering && plist.exists() {
        println!("\n{}", dim("try `aios daemon logs`"));
    }
    Ok(())
}

fn logs(lines: usize) -> Result<()> {
    let path = log_path();
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("no log at {}", path.display()))?;
    for line in text
        .lines()
        .rev()
        .take(lines)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
    {
        println!("{line}");
    }
    Ok(())
}
