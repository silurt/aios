//! `Harness` adapter for Codex.
//!
//! Driven as `codex exec --json`, which prints events to stdout as JSONL. Codex
//! uses a different vocabulary from Claude — its own item/message shapes rather
//! than Anthropic content blocks — which is the whole reason the normalized
//! [`RunEvent`] exists: everything above this line stops caring.

use aios_caps::ports::{Harness, RunSpec};
use aios_types::{HarnessId, MessageRole, RunEvent};
use serde_json::Value;

pub struct Codex;

impl Codex {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Codex {
    fn default() -> Self {
        Self::new()
    }
}

impl Harness for Codex {
    fn id(&self) -> HarnessId {
        HarnessId::Codex
    }

    fn binary(&self) -> &'static str {
        "codex"
    }

    fn command(&self, spec: &RunSpec) -> Vec<String> {
        let mut args = vec![
            "exec".into(),
            "--json".into(),
            // Codex resolves paths against its own cwd notion, so tell it
            // explicitly rather than relying on the spawned process's.
            "--cd".into(),
            spec.cwd.display().to_string(),
        ];
        if let Some(model) = &spec.model {
            args.extend(["--model".into(), model.clone()]);
        }
        args.push(spec.prompt.clone());
        args
    }

    fn resume_command(&self, spec: &RunSpec, session_ref: &str) -> Option<Vec<String>> {
        let mut args = vec!["exec".into(), "resume".into(), session_ref.to_string()];
        args.extend([
            "--json".into(),
            "--cd".into(),
            spec.cwd.display().to_string(),
        ]);
        args.push(spec.prompt.clone());
        Some(args)
    }

    fn translate(&self, line: &str) -> Vec<RunEvent> {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            return Vec::new();
        };

        // Codex has moved its event envelope more than once. Accept the type
        // tag from any of the places it has lived rather than pinning to one
        // and going silent on upgrade — a normalizer that quietly emits nothing
        // is worse than one that emits a Notice it did not understand.
        let kind = first_str(&[&v["type"], &v["msg"]["type"], &v["item"]["type"]]);

        match kind.as_deref() {
            Some("session.created") | Some("session_configured") | Some("thread.started") => {
                vec![RunEvent::Started {
                    session_ref: first_str(&[
                        &v["session_id"],
                        &v["msg"]["session_id"],
                        &v["thread_id"],
                    ]),
                    model: first_str(&[&v["model"], &v["msg"]["model"]]),
                    tools: Vec::new(),
                }]
            }
            Some("agent_message") | Some("assistant_message") | Some("item.completed") => {
                let text = first_str(&[
                    &v["message"],
                    &v["msg"]["message"],
                    &v["item"]["text"],
                    &v["text"],
                ]);
                match text {
                    Some(t) if !t.trim().is_empty() => vec![RunEvent::Message {
                        role: MessageRole::Assistant,
                        text: t.trim().to_string(),
                    }],
                    _ => Vec::new(),
                }
            }
            Some("agent_reasoning") | Some("reasoning") => {
                first_str(&[&v["text"], &v["msg"]["text"], &v["item"]["text"]])
                    .filter(|t| !t.trim().is_empty())
                    .map(|t| {
                        vec![RunEvent::Thinking {
                            text: t.trim().to_string(),
                        }]
                    })
                    .unwrap_or_default()
            }
            Some("exec_command_begin") | Some("command_execution.begin") => {
                vec![RunEvent::ToolUse {
                    id: first_str(&[&v["call_id"], &v["msg"]["call_id"]]),
                    name: "Bash".into(),
                    summary: first_str(&[&v["command"], &v["msg"]["command"]])
                        .unwrap_or_else(|| "(command)".into()),
                }]
            }
            Some("exec_command_end") | Some("command_execution.end") => {
                let code = v["exit_code"]
                    .as_i64()
                    .or_else(|| v["msg"]["exit_code"].as_i64());
                vec![RunEvent::ToolResult {
                    id: first_str(&[&v["call_id"], &v["msg"]["call_id"]]),
                    ok: code == Some(0),
                    summary: format!("exit {}", code.unwrap_or(-1)),
                }]
            }
            Some("error") | Some("stream_error") => vec![RunEvent::Failed {
                error: first_str(&[&v["message"], &v["msg"]["message"], &v["error"]])
                    .unwrap_or_else(|| "codex reported an error".into()),
            }],
            Some("task_complete") | Some("turn.completed") | Some("thread.completed") => {
                vec![RunEvent::Finished {
                    ok: true,
                    summary: first_str(&[
                        &v["last_agent_message"],
                        &v["msg"]["last_agent_message"],
                    ]),
                    cost_usd: None,
                    turns: None,
                    duration_ms: None,
                }]
            }
            _ => Vec::new(),
        }
    }
}

/// First of several candidate locations that holds a string.
fn first_str(candidates: &[&Value]) -> Option<String> {
    candidates
        .iter()
        .find_map(|v| v.as_str())
        .map(str::to_owned)
}
