//! Runs: one execution of a coding harness, and the normalized events it emits.

use crate::Timestamp;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

crate::newtype_id!(
    /// Identifies one run.
    RunId
);
crate::newtype_id!(
    /// Identifies one approval request.
    ApprovalId
);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum HarnessId {
    Claude,
    Codex,
}

impl HarnessId {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
        }
    }
}

impl std::str::FromStr for HarnessId {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "claude" => Ok(Self::Claude),
            "codex" => Ok(Self::Codex),
            other => Err(format!(
                "unknown harness {other:?}; expected claude or codex"
            )),
        }
    }
}

/// Where a run is in its life.
///
/// `Parked` is the one that matters. A run that hits an approval nobody
/// answers must not die: it stops at the gate, keeps its workspace and its
/// transcript, and can be resumed once a decision arrives (plan §7.1). Without
/// push notifications (§13.3) that is the normal case, not the exception.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum RunStatus {
    Running,
    /// Waiting on an approval decision.
    AwaitingApproval,
    /// Stopped at a gate that expired. Resumable.
    Parked,
    Succeeded,
    Failed,
    /// Stopped by a human.
    Interrupted,
}

impl RunStatus {
    /// Whether the run has stopped for good. `Parked` is deliberately *not*
    /// terminal — it is waiting for a person, not finished.
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Interrupted)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Run {
    pub id: RunId,
    pub harness: HarnessId,
    /// Registry slug, when the run is scoped to a registered project.
    pub project: Option<String>,
    /// Working directory the harness was given.
    pub cwd: String,
    /// The task as written by whoever started it.
    pub prompt: String,
    pub status: RunStatus,
    /// The harness's own session identifier, for resuming through it.
    pub session_ref: Option<String>,
    pub model: Option<String>,
    /// Highest event sequence written for this run — the cursor a client
    /// resumes from (§13.2).
    pub last_seq: u64,
    /// OS process id of the harness, while it is running.
    ///
    /// Stored rather than held in memory so a run started by a previous daemon
    /// lifetime can still be interrupted. Only trustworthy while `status` is
    /// `Running` — pids are reused, so anything else must not signal it.
    pub pid: Option<u32>,
    pub exit_code: Option<i32>,
    pub error: Option<String>,
    pub cost_usd: Option<f64>,
    pub turns: Option<u32>,
    // serde, utoipa and schemars each need telling separately that this is an
    // RFC 3339 string on the wire — none of them infers it from the others.
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    #[schemars(with = "String")]
    pub started_at: Timestamp,
    #[serde(with = "time::serde::rfc3339::option")]
    #[schema(value_type = Option<String>, format = DateTime)]
    #[schemars(with = "Option<String>")]
    pub ended_at: Option<Timestamp>,
}

/// A harness event, normalized.
///
/// Internally tagged so it generates a real Swift enum (§15). Every harness
/// maps onto this, so no consumer — CLI, UI, channel — ever branches on which
/// harness produced it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema, JsonSchema)]
// `rename_all` renames the *variants*; fields inside them need
// `rename_all_fields`. Without it `sessionRef` ships as `session_ref` and the
// wire format is camelCase in the tag and snake_case in the payload.
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[schemars(rename_all = "camelCase")]
pub enum RunEvent {
    /// The harness came up and reported its session.
    Started {
        session_ref: Option<String>,
        model: Option<String>,
        /// Tool names the harness says it has.
        #[serde(default)]
        tools: Vec<String>,
    },
    /// Prose from the assistant or the user.
    Message {
        role: MessageRole,
        text: String,
    },
    /// The model asked to use a tool.
    ToolUse {
        id: Option<String>,
        name: String,
        /// A one-line rendering of the input. The full input lives in the
        /// approval when one is raised; putting it here would make every
        /// transcript enormous for no benefit.
        summary: String,
    },
    ToolResult {
        id: Option<String>,
        ok: bool,
        summary: String,
    },
    Thinking {
        text: String,
    },
    /// An approval was raised, decided, or expired.
    Approval {
        id: ApprovalId,
        tool: String,
        state: ApprovalState,
    },
    /// Something worth recording that is not part of the conversation: a
    /// retry, a rate limit, a hook firing.
    Notice {
        detail: String,
    },
    Finished {
        ok: bool,
        summary: Option<String>,
        cost_usd: Option<f64>,
        turns: Option<u32>,
        duration_ms: Option<u64>,
    },
    Failed {
        error: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum MessageRole {
    Assistant,
    User,
    System,
}

// ---------------------------------------------------------------- approvals

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ApprovalState {
    Pending,
    Approved,
    Denied,
    /// Timed out with nobody answering. The run parks; the decision can still
    /// be made later.
    Expired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Decider {
    /// Settled by a pre-authorisation rule, without asking anyone.
    Policy,
    User,
    /// Nobody answered in time.
    Timeout,
}

/// A permission decision, lifted out of the harness.
///
/// Left as an inline prompt on a pty, a permission question is answerable only
/// by whoever is staring at that terminal — which makes every other surface
/// useless for anything but watching. As a persisted row with an id, the CLI,
/// the Mac app, the phone, or a policy can all decide it (plan §7.1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Approval {
    pub id: ApprovalId,
    pub run_id: RunId,
    pub project: Option<String>,
    /// Tool the harness wants to use, e.g. `Bash`, `Edit`.
    pub tool: String,
    /// One line, for a list or a notification.
    pub summary: String,
    /// The full request, verbatim, for someone deciding.
    pub detail: Option<String>,
    pub state: ApprovalState,
    pub decided_by: Option<Decider>,
    /// Which rule settled it, when policy did.
    pub rule: Option<String>,
    pub reason: Option<String>,
    // serde, utoipa and schemars each need telling separately that this is an
    // RFC 3339 string on the wire — none of them infers it from the others.
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    #[schemars(with = "String")]
    pub requested_at: Timestamp,
    // serde, utoipa and schemars each need telling separately that this is an
    // RFC 3339 string on the wire — none of them infers it from the others.
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    #[schemars(with = "String")]
    pub expires_at: Timestamp,
    #[serde(with = "time::serde::rfc3339::option")]
    #[schema(value_type = Option<String>, format = DateTime)]
    #[schemars(with = "Option<String>")]
    pub decided_at: Option<Timestamp>,
}

impl Approval {
    pub fn is_pending(&self) -> bool {
        self.state == ApprovalState::Pending
    }
}
