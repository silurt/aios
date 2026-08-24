//! `aios approval gate` — the bridge between a harness's permission prompt and
//! AIOS's approval objects.
//!
//! This is the mechanism §7.1 needs. Claude Code has no `--permission-prompt-tool`
//! in this version, so MCP cannot serve the role; a **PreToolUse hook** can, and
//! its contract was established empirically rather than assumed:
//!
//! - stdin carries `{session_id, cwd, tool_name, tool_input, tool_use_id, …}`
//! - stdout carries a `hookSpecificOutput` with `permissionDecision`
//!   (`allow` | `deny` | `ask`), which the harness honours.
//!
//! The command blocks while a decision is outstanding. That is the point: the
//! harness waits at the gate rather than failing, and if nobody answers before
//! the deadline the run is parked instead of killed.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::io::Read;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
struct HookInput {
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    tool_name: String,
    #[serde(default)]
    tool_input: serde_json::Value,
    #[serde(default)]
    cwd: Option<String>,
}

/// How often to re-check for a decision while blocked.
///
/// A second is imperceptible to a human and cheap for us: the check is one
/// small file read, and a run waiting at a gate is not doing anything else.
const POLL: std::time::Duration = std::time::Duration::from_secs(1);

pub fn run() -> Result<()> {
    let mut raw = String::new();
    std::io::stdin()
        .read_to_string(&mut raw)
        .context("reading the hook payload")?;
    let input: HookInput =
        serde_json::from_str(&raw).context("the hook payload was not the expected shape")?;

    let policy = crate::app::policy()?;
    let summary = aios_claude::summarize_tool_input(&input.tool_input);
    let decision = policy.decide(&input.tool_name, &summary);

    use aios_runs::Verdict;
    match decision.verdict {
        Verdict::Allow => emit(
            "allow",
            &rule_reason("allowed by", decision.rule.as_deref()),
        ),
        Verdict::Deny => emit("deny", &rule_reason("denied by", decision.rule.as_deref())),
        Verdict::Ask => ask(&input, &summary, &policy),
    }
}

fn ask(input: &HookInput, summary: &str, policy: &aios_runs::Policy) -> Result<()> {
    let approvals = aios_runs::Approvals::open()?;
    let supervisor = aios_runs::Supervisor::open(policy.clone())?;

    // Correlate to a run by the harness's own session id. A harness a person
    // started by hand has no run here, which is fine — the gate still works,
    // there is simply nothing to park.
    let runs = supervisor.all().unwrap_or_default();
    let run = runs
        .iter()
        .find(|r| {
            input.session_id.is_some() && r.session_ref.as_deref() == input.session_id.as_deref()
        })
        // Fall back to the newest live run in this directory. The session id is
        // the precise match, but a harness that has not reported its session
        // yet would otherwise orphan its first approval — which is exactly the
        // one raised earliest in a run.
        .or_else(|| {
            runs.iter().find(|r| {
                !r.status.is_terminal() && input.cwd.as_deref().is_some_and(|cwd| r.cwd == cwd)
            })
        })
        .cloned();

    let approval = approvals.raise(
        policy,
        aios_runs::Request {
            run_id: run
                .as_ref()
                .map(|r| r.id.clone())
                .unwrap_or_else(|| aios_types::RunId("unattached".into())),
            project: run.as_ref().and_then(|r| r.project.clone()),
            tool: input.tool_name.clone(),
            summary: summary.to_string(),
            // The full input goes here rather than into the transcript: this is
            // where somebody actually reads it before deciding.
            detail: serde_json::to_string_pretty(&input.tool_input).ok(),
        },
    )?;

    // Say where it is on stderr — the harness shows this, and it is the only
    // hint a person watching gets that something needs them.
    eprintln!(
        "aios: waiting for approval {} ({} {})",
        approval.id, input.tool_name, summary
    );

    loop {
        match approvals.outcome(approval.id.as_str(), time::OffsetDateTime::now_utc())? {
            Some(true) => return emit("allow", "approved"),
            Some(false) => {
                // Deadline passed with nobody answering: park the run so it can
                // be resumed, rather than letting it read as a failure.
                if let Some(run) = &run {
                    let expired = approvals.get(approval.id.as_str())?.state
                        == aios_types::ApprovalState::Expired;
                    if expired {
                        let _ = supervisor.park(
                            run.id.as_str(),
                            &format!("waiting on approval {}", approval.id),
                        );
                    }
                }
                return emit("deny", &format!("approval {} was not granted", approval.id));
            }
            None => std::thread::sleep(POLL),
        }
    }
}

fn rule_reason(verb: &str, rule: Option<&str>) -> String {
    match rule {
        Some(name) => format!("{verb} policy rule {name:?}"),
        None => format!("{verb} the policy default"),
    }
}

/// Emit the decision in the shape the harness expects.
///
/// stdout is the hook's channel, so nothing else may be printed here.
fn emit(decision: &str, reason: &str) -> Result<()> {
    println!(
        "{}",
        serde_json::json!({
            "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "permissionDecision": decision,
                "permissionDecisionReason": reason,
            }
        })
    );
    Ok(())
}
