//! `aios run …` — starting and inspecting runs.

use crate::render::{bold, dim, green, red, yellow};
use anyhow::{Context, Result};
use clap::Subcommand;
use std::path::PathBuf;

#[derive(Subcommand)]
pub enum RunCommand {
    /// Start a harness on a task
    Start {
        /// What to do
        task: String,
        #[arg(long, short)]
        project: Option<String>,
        /// claude or codex
        #[arg(long, short = 'H', default_value = "claude")]
        harness: String,
        #[arg(long, short)]
        model: Option<String>,
        /// Print normalized events as they arrive instead of only the result
        #[arg(long)]
        stream: bool,
    },
    /// List runs, newest first
    #[command(visible_alias = "ls")]
    List {
        #[arg(long, short, default_value_t = 20)]
        limit: usize,
    },
    /// Show one run
    Show { run: String },
    /// Continue a parked run through its harness session
    Resume {
        run: String,
        /// Steer it somewhere new instead of repeating the original task
        #[arg(long, short)]
        task: Option<String>,
        #[arg(long)]
        stream: bool,
    },
    /// Replay a run's transcript
    Events {
        run: String,
        /// Resume from this sequence number (§13.2 cursor)
        #[arg(long, default_value_t = 0)]
        since: u64,
    },
}

pub fn run(cmd: RunCommand, json: bool) -> Result<()> {
    match cmd {
        RunCommand::Start {
            task,
            project,
            harness,
            model,
            stream,
        } => start(task, project, &harness, model, stream, json),
        RunCommand::List { limit } => list(limit, json),
        RunCommand::Show { run } => show(&run, json),
        RunCommand::Resume { run, task, stream } => resume(&run, task.as_deref(), stream, json),
        RunCommand::Events { run, since } => events(&run, since, json),
    }
}

fn supervisor() -> Result<aios_runs::Supervisor> {
    Ok(aios_runs::Supervisor::open(crate::app::policy()?)?)
}

fn start(
    task: String,
    project: Option<String>,
    harness: &str,
    model: Option<String>,
    stream: bool,
    json: bool,
) -> Result<()> {
    let harness_id: aios_types::HarnessId =
        harness.parse().map_err(|e: String| anyhow::anyhow!(e))?;

    // Resolve the working directory through the registry when a project is
    // named, so `--project foo` works from anywhere.
    let registry = aios_core::Registry::open()?;
    let (cwd, slug) = match &project {
        Some(needle) => {
            let p = registry.resolve(needle)?;
            (PathBuf::from(&p.path), Some(p.slug))
        }
        None => {
            let cwd = std::env::current_dir()?;
            let slug = registry
                .resolve(&cwd.display().to_string())
                .ok()
                .map(|p| p.slug);
            (cwd, slug)
        }
    };

    let supervisor = supervisor()?;
    let start = aios_runs::supervisor::StartRun {
        harness: aios_runs::supervisor::harness_for(harness_id),
        prompt: task,
        cwd,
        project: slug,
        model,
    };

    if !json && !stream {
        println!("{} {}", dim("running"), bold(harness_id.as_str()));
    }

    let completed = supervisor.run(start, |event| {
        if stream {
            // One JSON object per line: the same shape a client reading the
            // event stream sees, so `--stream | jq` and a UI agree.
            if let Ok(line) = serde_json::to_string(event) {
                println!("{line}");
            }
        } else if !json {
            print_event(&event.data);
        }
    })?;

    if json {
        println!("{}", serde_json::to_string_pretty(&completed)?);
    } else if !stream {
        println!();
        println!("{} {}", dim("run"), bold(completed.id.as_str()));
    }
    Ok(())
}

fn print_event(event: &aios_types::RunEvent) {
    use aios_types::RunEvent as E;
    match event {
        E::Started { model, .. } => {
            println!("{} {}", dim("started"), dim(model.as_deref().unwrap_or("")))
        }
        E::Message { role, text } => match role {
            aios_types::MessageRole::Assistant => println!("{text}"),
            _ => println!("{}", dim(text)),
        },
        E::Thinking { .. } => {}
        E::ToolUse { name, summary, .. } => {
            println!("{} {} {}", yellow("→"), bold(name), dim(summary))
        }
        E::ToolResult { ok, summary, .. } => {
            let mark = if *ok { green("✓") } else { red("✗") };
            println!("  {mark} {}", dim(summary));
        }
        E::Approval { tool, state, .. } => {
            println!(
                "{} {tool} {}",
                yellow("approval"),
                dim(&format!("{state:?}"))
            )
        }
        E::Notice { detail } => println!("{}", dim(detail)),
        E::Finished {
            ok,
            cost_usd,
            turns,
            ..
        } => {
            let mark = if *ok { green("done") } else { red("failed") };
            let cost = cost_usd.map(|c| format!(" ${c:.4}")).unwrap_or_default();
            let turns = turns.map(|t| format!(" {t} turns")).unwrap_or_default();
            println!("{mark}{}{}", dim(&turns), dim(&cost));
        }
        E::Failed { error } => println!("{} {error}", red("error")),
    }
}

fn resume(needle: &str, task: Option<&str>, stream: bool, json: bool) -> Result<()> {
    let supervisor = supervisor()?;
    let existing = supervisor.get(needle)?;

    if !json && !stream {
        println!(
            "{} {} {}",
            dim("resuming"),
            bold(existing.id.as_str()),
            dim(&format!("from event {}", existing.last_seq))
        );
    }

    let completed = supervisor.resume(existing.id.as_str(), task, |event| {
        if stream {
            if let Ok(line) = serde_json::to_string(event) {
                println!("{line}");
            }
        } else if !json {
            print_event(&event.data);
        }
    })?;

    if json {
        println!("{}", serde_json::to_string_pretty(&completed)?);
    }
    Ok(())
}

fn list(limit: usize, json: bool) -> Result<()> {
    let runs: Vec<_> = supervisor()?.all()?.into_iter().take(limit).collect();
    if json {
        println!("{}", serde_json::to_string_pretty(&runs)?);
        return Ok(());
    }
    if runs.is_empty() {
        println!("{}", dim("no runs yet — try `aios run start \"…\"`"));
        return Ok(());
    }
    for r in &runs {
        println!(
            "{} {} {} {}",
            status_badge(r.status),
            bold(r.id.as_str()),
            dim(r.harness.as_str()),
            truncate(&r.prompt, 60)
        );
    }
    Ok(())
}

fn status_badge(status: aios_types::RunStatus) -> String {
    use aios_types::RunStatus as S;
    match status {
        S::Running => yellow("running "),
        S::AwaitingApproval => yellow("approval"),
        S::Parked => yellow("parked  "),
        S::Succeeded => green("done    "),
        S::Failed => red("failed  "),
        S::Interrupted => dim("stopped "),
    }
}

fn show(needle: &str, json: bool) -> Result<()> {
    let run = supervisor()?.get(needle)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&run)?);
        return Ok(());
    }
    let field = |k: &str, v: &str| println!("{} {v}", dim(&format!("{k:<12}")));
    println!("{} {}", bold(run.id.as_str()), status_badge(run.status));
    field("harness", run.harness.as_str());
    field("project", run.project.as_deref().unwrap_or("—"));
    field("cwd", &run.cwd);
    field("model", run.model.as_deref().unwrap_or("—"));
    field("session", run.session_ref.as_deref().unwrap_or("—"));
    field("events", &run.last_seq.to_string());
    if let Some(cost) = run.cost_usd {
        field("cost", &format!("${cost:.4}"));
    }
    if let Some(error) = &run.error {
        field("error", error);
    }
    println!("\n{}", run.prompt);
    Ok(())
}

fn events(needle: &str, since: u64, json: bool) -> Result<()> {
    let supervisor = supervisor()?;
    let run = supervisor.get(needle)?;
    let events = supervisor
        .events(run.id.as_str(), since, 10_000)
        .with_context(|| format!("reading events for {}", run.id))?;

    if json {
        println!("{}", serde_json::to_string_pretty(&events)?);
        return Ok(());
    }
    for event in &events {
        print!("{} ", dim(&format!("{:>4}", event.seq)));
        print_event(&event.data);
    }
    Ok(())
}

fn truncate(s: &str, max: usize) -> String {
    let s = s.replace('\n', " ");
    if s.chars().count() <= max {
        return s;
    }
    format!("{}…", s.chars().take(max).collect::<String>())
}
