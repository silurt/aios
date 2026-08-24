//! `Harness` adapter for Claude Code.
//!
//! Driven as `claude -p --output-format stream-json`, which emits one JSON
//! object per line. The event shapes here were read off a real run rather than
//! inferred: `system` carries `init`, hook lifecycle and retries; `assistant`
//! and `user` carry Anthropic message content blocks; `result` closes the run
//! with cost, turns and any permission denials.

use aios_caps::ports::{Harness, RunSpec};
use aios_types::{HarnessId, MessageRole, RunEvent};
use serde_json::Value;

pub struct Claude;

impl Claude {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Claude {
    fn default() -> Self {
        Self::new()
    }
}

impl Harness for Claude {
    fn id(&self) -> HarnessId {
        HarnessId::Claude
    }

    fn binary(&self) -> &'static str {
        "claude"
    }

    fn command(&self, spec: &RunSpec) -> Vec<String> {
        let mut args = vec![
            "-p".into(),
            spec.prompt.clone(),
            "--output-format".into(),
            "stream-json".into(),
            // stream-json refuses to emit the full event stream without it.
            "--verbose".into(),
        ];
        if let Some(model) = &spec.model {
            args.extend(["--model".into(), model.clone()]);
        }
        if !spec.allowed_tools.is_empty() {
            args.push("--allowed-tools".into());
            args.extend(spec.allowed_tools.clone());
        }
        if !spec.disallowed_tools.is_empty() {
            args.push("--disallowed-tools".into());
            args.extend(spec.disallowed_tools.clone());
        }
        args
    }

    fn resume_command(&self, spec: &RunSpec, session_ref: &str) -> Option<Vec<String>> {
        let mut args = self.command(spec);
        args.extend(["--resume".into(), session_ref.to_string()]);
        Some(args)
    }

    fn translate(&self, line: &str) -> Vec<RunEvent> {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            return Vec::new();
        };
        match v["type"].as_str().unwrap_or_default() {
            "system" => translate_system(&v),
            "assistant" => translate_message(&v, MessageRole::Assistant),
            "user" => translate_message(&v, MessageRole::User),
            "result" => vec![translate_result(&v)],
            // Rate limits are operationally interesting but not conversation.
            "rate_limit_event" => vec![RunEvent::Notice {
                detail: "rate limited".into(),
            }],
            _ => Vec::new(),
        }
    }
}

fn translate_system(v: &Value) -> Vec<RunEvent> {
    match v["subtype"].as_str().unwrap_or_default() {
        "init" => vec![RunEvent::Started {
            session_ref: v["session_id"].as_str().map(str::to_owned),
            model: v["model"].as_str().map(str::to_owned),
            tools: v["tools"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|t| t.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default(),
        }],
        "api_retry" => vec![RunEvent::Notice {
            detail: format!(
                "api retry {}/{}: {}",
                v["attempt"],
                v["max_retries"],
                v["error"].as_str().unwrap_or("unknown")
            ),
        }],
        // Hook chatter is high-volume and says nothing about the task. Dropping
        // it here keeps transcripts about the work rather than the plumbing.
        _ => Vec::new(),
    }
}

fn translate_message(v: &Value, role: MessageRole) -> Vec<RunEvent> {
    let Some(blocks) = v["message"]["content"].as_array() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for block in blocks {
        match block["type"].as_str().unwrap_or_default() {
            "text" => {
                let text = block["text"].as_str().unwrap_or_default().trim();
                if !text.is_empty() {
                    out.push(RunEvent::Message {
                        role,
                        text: text.to_string(),
                    });
                }
            }
            "thinking" => {
                let text = block["thinking"].as_str().unwrap_or_default().trim();
                if !text.is_empty() {
                    out.push(RunEvent::Thinking {
                        text: text.to_string(),
                    });
                }
            }
            "tool_use" => out.push(RunEvent::ToolUse {
                id: block["id"].as_str().map(str::to_owned),
                name: block["name"].as_str().unwrap_or("unknown").to_string(),
                summary: summarize_tool_input(&block["input"]),
            }),
            "tool_result" => out.push(RunEvent::ToolResult {
                id: block["tool_use_id"].as_str().map(str::to_owned),
                ok: !block["is_error"].as_bool().unwrap_or(false),
                summary: truncate(&stringify(&block["content"]), 200),
            }),
            _ => {}
        }
    }
    out
}

fn translate_result(v: &Value) -> RunEvent {
    let ok = !v["is_error"].as_bool().unwrap_or(false) && v["subtype"].as_str() == Some("success");

    // Permission denials are how a refused tool surfaces in the result. They
    // are worth carrying into the summary: a run that "succeeded" having been
    // blocked from everything it tried is not a success anyone wants reported
    // silently.
    let denials = v["permission_denials"]
        .as_array()
        .map(|a| a.len())
        .unwrap_or(0);
    let mut summary = v["result"].as_str().map(str::to_owned);
    if denials > 0 {
        let note = format!("{denials} permission denial(s)");
        summary = Some(match summary {
            Some(s) => format!("{s}\n\n[{note}]"),
            None => note,
        });
    }

    RunEvent::Finished {
        ok,
        summary,
        cost_usd: v["total_cost_usd"].as_f64(),
        turns: v["num_turns"].as_u64().map(|n| n as u32),
        duration_ms: v["duration_ms"].as_u64(),
    }
}

/// Render a tool's input as one line.
///
/// The full input is not kept in the transcript — an `Edit` carries whole file
/// contents, and every transcript would balloon. When a tool needs a decision
/// the full input goes on the approval instead, where someone will actually
/// read it.
pub fn summarize_tool_input(input: &Value) -> String {
    for key in [
        "command",
        "file_path",
        "pattern",
        "path",
        "url",
        "query",
        "description",
    ] {
        if let Some(s) = input[key].as_str() {
            return truncate(s, 160);
        }
    }
    truncate(&stringify(input), 160)
}

fn stringify(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn truncate(s: &str, max: usize) -> String {
    let s = s.trim().replace('\n', " ");
    if s.chars().count() <= max {
        return s;
    }
    let kept: String = s.chars().take(max).collect();
    format!("{kept}…")
}
