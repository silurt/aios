//! `aios run …` — starting and inspecting runs.

use crate::render::{bold, dim, green, red, yellow};
use anyhow::Result;
use clap::Subcommand;

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

fn client() -> Result<crate::client::Client> {
    crate::client::Client::connect()
}

fn start(
    task: String,
    project: Option<String>,
    harness: &str,
    model: Option<String>,
    stream: bool,
    json: bool,
) -> Result<()> {
    // Parse before talking to the daemon, so a typo is an immediate local
    // error rather than a round trip.
    let harness_id: aios_types::HarnessId =
        harness.parse().map_err(|e: String| anyhow::anyhow!(e))?;
    let client = client()?;

    let project = match project {
        Some(p) => Some(p),
        // Name the current directory explicitly: the daemon has its own working
        // directory and must not be asked to guess ours.
        None => std::env::current_dir()
            .ok()
            .map(|c| c.display().to_string()),
    };

    let started = client.post(
        "/api/runs",
        &serde_json::json!({
            "prompt": task,
            "project": project,
            "harness": harness_id,
            "model": model,
        }),
    )?;
    let id = started["id"].as_str().unwrap_or_default().to_string();

    if !json && !stream {
        println!(
            "{} {} {}",
            dim("running"),
            bold(harness_id.as_str()),
            dim(&id)
        );
    }

    // The run happens in the daemon; follow it over the resumable stream. A
    // dropped connection costs nothing — reconnecting replays from the cursor.
    client.stream(&format!("/api/runs/{id}/stream"), |record| {
        render_record(&record, stream, json)
    })?;

    let finished = client.get(&format!("/api/runs/{id}"))?;
    if json {
        println!("{}", serde_json::to_string_pretty(&finished)?);
    }
    Ok(())
}

fn resume(needle: &str, task: Option<&str>, stream: bool, json: bool) -> Result<()> {
    let client = client()?;
    let existing = client.get(&format!("/api/runs/{needle}"))?;
    let id = existing["id"].as_str().unwrap_or(needle).to_string();
    let since = existing["lastSeq"].as_u64().unwrap_or(0);

    if !json && !stream {
        println!(
            "{} {} {}",
            dim("resuming"),
            bold(&id),
            dim(&format!("from event {since}"))
        );
    }

    client.post(
        &format!("/api/runs/{id}/resume"),
        &serde_json::json!({ "task": task }),
    )?;

    // Resume from where the run left off, so the transcript is not replayed.
    client.stream(&format!("/api/runs/{id}/stream?since={since}"), |record| {
        render_record(&record, stream, json)
    })?;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&client.get(&format!("/api/runs/{id}"))?)?
        );
    }
    Ok(())
}

/// How a streamed record is shown.
///
/// `--stream` prints the record verbatim — the same shape a client reading the
/// event stream sees, so `--stream | jq` and a UI agree. Otherwise it is
/// rendered for a human, unless `--json` is asking for the final run object
/// only.
fn render_record(record: &serde_json::Value, stream: bool, json: bool) {
    if stream {
        if let Ok(line) = serde_json::to_string(record) {
            println!("{line}");
        }
    } else if !json
        && let Ok(event) = serde_json::from_value::<aios_types::RunEvent>(record["data"].clone())
    {
        print_event(&event);
    }
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

fn list(limit: usize, json: bool) -> Result<()> {
    let listed = client()?.get("/api/runs")?;
    let runs: Vec<serde_json::Value> = listed
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .take(limit)
        .collect();
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
            status_badge_str(r["status"].as_str().unwrap_or("")),
            bold(r["id"].as_str().unwrap_or("")),
            dim(r["harness"].as_str().unwrap_or("")),
            truncate(r["prompt"].as_str().unwrap_or(""), 60)
        );
    }
    Ok(())
}

/// Render a status from the wire form. The CLI reads runs as JSON now, so it
/// branches on the serialized name rather than the enum.
fn status_badge_str(status: &str) -> String {
    match status {
        "running" => yellow("running "),
        "awaitingApproval" => yellow("approval"),
        "parked" => yellow("parked  "),
        "succeeded" => green("done    "),
        "failed" => red("failed  "),
        "interrupted" => dim("stopped "),
        other => dim(other),
    }
}

fn show(needle: &str, json: bool) -> Result<()> {
    let run = client()?.get(&format!("/api/runs/{needle}"))?;
    if json {
        println!("{}", serde_json::to_string_pretty(&run)?);
        return Ok(());
    }
    let field = |k: &str, v: &str| println!("{} {v}", dim(&format!("{k:<12}")));
    let s = |k: &str| run[k].as_str().unwrap_or("—").to_string();
    println!(
        "{} {}",
        bold(&s("id")),
        status_badge_str(run["status"].as_str().unwrap_or(""))
    );
    field("harness", &s("harness"));
    field("project", &s("project"));
    field("cwd", &s("cwd"));
    field("model", &s("model"));
    field("session", &s("sessionRef"));
    field("events", &run["lastSeq"].to_string());
    if let Some(cost) = run["costUsd"].as_f64() {
        field("cost", &format!("${cost:.4}"));
    }
    if let Some(error) = run["error"].as_str() {
        field("error", error);
    }
    println!("\n{}", s("prompt"));
    Ok(())
}

fn events(needle: &str, since: u64, json: bool) -> Result<()> {
    let client = client()?;
    let records = client.get(&format!("/api/runs/{needle}/events?since={since}"))?;
    if json {
        println!("{}", serde_json::to_string_pretty(&records)?);
        return Ok(());
    }
    for record in records.as_array().cloned().unwrap_or_default() {
        print!("{} ", dim(&format!("{:>4}", record["seq"])));
        match serde_json::from_value::<aios_types::RunEvent>(record["data"].clone()) {
            Ok(event) => print_event(&event),
            // A record this build cannot interpret is shown rather than
            // dropped — the store already skips version mismatches, and
            // silently printing nothing would look like an empty transcript.
            Err(_) => println!("{}", dim("(unreadable event)")),
        }
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
