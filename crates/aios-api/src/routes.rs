//! The API surface.
//!
//! Every route is a thin shell over something that already exists: the
//! capability registry, the supervisor, the approval store. Nothing here holds
//! domain logic — that is the §1.1 rule stated from the server's side, and it
//! is what lets the CLI, the Mac app and the phone all be equal clients.

use crate::error::{ApiFailure, ApiResult};
use crate::state::Shared;
use aios_types::{Approval, Run, RunEvent, VersionInfo};
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::response::sse::{Event, Sse};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use std::time::Duration;

pub fn router(state: Shared) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/version", get(version))
        .route("/api/capabilities", get(list_capabilities))
        .route("/api/capabilities/{name}", post(call_capability))
        .route("/api/runs", get(list_runs).post(start_run))
        .route("/api/runs/{id}", get(get_run))
        .route("/api/runs/{id}/events", get(run_events))
        .route("/api/runs/{id}/stream", get(stream_run))
        .route("/api/runs/{id}/resume", post(resume_run))
        .route("/api/runs/{id}/interrupt", post(interrupt_run))
        .route("/api/approvals", get(list_approvals))
        .route("/api/approvals/{id}", get(get_approval))
        .route("/api/approvals/{id}/decide", post(decide_approval))
        .layer(axum::middleware::from_fn(crate::version::negotiate))
        .with_state(state)
}

async fn health() -> impl IntoResponse {
    (StatusCode::OK, Json(serde_json::json!({ "ok": true })))
}

async fn version() -> Json<VersionInfo> {
    Json(VersionInfo::current())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CapabilitySummary {
    name: String,
    summary: String,
    effect: &'static str,
    input_schema: serde_json::Value,
}

async fn list_capabilities(State(state): State<Shared>) -> Json<Vec<CapabilitySummary>> {
    Json(
        state
            .capabilities
            .iter()
            .map(|c| CapabilitySummary {
                name: c.name.to_string(),
                summary: c.summary.to_string(),
                effect: if c.effect.is_write() { "write" } else { "read" },
                input_schema: c.input_schema.clone(),
            })
            .collect(),
    )
}

/// Invoke a capability by name.
///
/// Runs on `spawn_blocking` for the same reason the MCP server does: handlers
/// shell out to `bd` and `git`, and a slow one on a reactor thread would stall
/// every other request including the event streams.
async fn call_capability(
    State(state): State<Shared>,
    Path(name): Path<String>,
    Json(input): Json<serde_json::Value>,
) -> ApiResult<Json<serde_json::Value>> {
    let result =
        tokio::task::spawn_blocking(move || state.capabilities.call(&state.context, &name, input))
            .await
            .map_err(|e| {
                ApiFailure(aios_types::ApiError {
                    kind: aios_types::ErrorKind::Internal,
                    message: format!("capability task failed: {e}"),
                })
            })??;
    Ok(Json(result))
}

async fn list_runs(State(state): State<Shared>) -> ApiResult<Json<Vec<Run>>> {
    Ok(Json(state.supervisor.all()?))
}

async fn get_run(State(state): State<Shared>, Path(id): Path<String>) -> ApiResult<Json<Run>> {
    Ok(Json(state.supervisor.get(&id)?))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StartRunBody {
    prompt: String,
    #[serde(default)]
    project: Option<String>,
    #[serde(default)]
    harness: Option<aios_types::HarnessId>,
    #[serde(default)]
    model: Option<String>,
}

/// Start a run and return immediately.
///
/// A run takes minutes; holding the request open for it would tie a client's
/// connection to the work and make every disconnect look like a failure. The
/// response carries the run id, and the client follows `/stream` — which is
/// resumable, so a dropped connection costs nothing.
async fn start_run(
    State(state): State<Shared>,
    Json(body): Json<StartRunBody>,
) -> ApiResult<(StatusCode, Json<Run>)> {
    let (cwd, slug) = match &body.project {
        Some(needle) => {
            let p = state.context.registry.resolve(needle)?;
            (std::path::PathBuf::from(&p.path), Some(p.slug))
        }
        None => (std::env::current_dir().map_err(aios_core::Error::Io)?, None),
    };

    let harness = body.harness.unwrap_or(aios_types::HarnessId::Claude);
    let started = state.supervisor.prepare(aios_runs::supervisor::StartRun {
        harness: aios_runs::supervisor::harness_for(harness),
        prompt: body.prompt,
        cwd,
        project: slug,
        model: body.model,
    })?;

    // The work itself is blocking and long; detach it so the request returns.
    let run_id = started.run.id.clone();
    let state2 = state.clone();
    tokio::task::spawn_blocking(move || {
        let _ = state2.supervisor.execute(started, |_| {});
    });

    Ok((
        StatusCode::ACCEPTED,
        Json(state.supervisor.get(run_id.as_str())?),
    ))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResumeBody {
    #[serde(default)]
    task: Option<String>,
}

async fn resume_run(
    State(state): State<Shared>,
    Path(id): Path<String>,
    Json(body): Json<ResumeBody>,
) -> ApiResult<(StatusCode, Json<Run>)> {
    let run = state.supervisor.get(&id)?;
    let state2 = state.clone();
    let id2 = id.clone();
    tokio::task::spawn_blocking(move || {
        let _ = state2.supervisor.resume(&id2, body.task.as_deref(), |_| {});
    });
    Ok((StatusCode::ACCEPTED, Json(run)))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EventQuery {
    #[serde(default)]
    since: u64,
    #[serde(default = "default_limit")]
    limit: usize,
}

fn default_limit() -> usize {
    1_000
}

async fn run_events(
    State(state): State<Shared>,
    Path(id): Path<String>,
    Query(q): Query<EventQuery>,
) -> ApiResult<Json<Vec<aios_core::store::Sequenced<RunEvent>>>> {
    Ok(Json(state.supervisor.events(&id, q.since, q.limit)?))
}

/// Live event stream, resumable.
///
/// The cursor is the event sequence number, sent as the SSE event id, so a
/// browser or `URLSession` reconnect carries `Last-Event-ID` automatically and
/// picks up exactly where it stopped (§13.2). An explicit `?since=` wins over
/// the header for clients that track it themselves.
///
/// New events are found by polling the run's JSONL log rather than by an
/// in-process broadcast channel. That is deliberate: a run started by the CLI
/// lives in a *different process*, and a channel would never see it. Polling a
/// file is the only mechanism that works regardless of who spawned the run.
async fn stream_run(
    State(state): State<Shared>,
    Path(id): Path<String>,
    Query(q): Query<EventQuery>,
    headers: HeaderMap,
) -> Sse<impl futures_core::Stream<Item = Result<Event, Infallible>>> {
    let resume_from = if q.since > 0 {
        q.since
    } else {
        headers
            .get("last-event-id")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(0)
    };

    let stream = async_stream::stream! {
        let mut cursor = resume_from;
        loop {
            let batch = state.supervisor.events(&id, cursor, 200).unwrap_or_default();
            for record in batch {
                cursor = record.seq;
                let data = serde_json::to_string(&record).unwrap_or_default();
                yield Ok(Event::default().id(record.seq.to_string()).data(data));
            }

            // Stop once the run is finished *and* the log is drained, so a
            // client is not left holding an open connection to a dead run.
            if let Ok(run) = state.supervisor.get(&id)
                && run.status.is_terminal()
                && cursor >= run.last_seq
            {
                yield Ok(Event::default().event("done").data("{}"));
                break;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    };

    Sse::new(stream).keep_alive(axum::response::sse::KeepAlive::default())
}

/// Stop a running agent.
async fn interrupt_run(
    State(state): State<Shared>,
    Path(id): Path<String>,
) -> ApiResult<Json<Run>> {
    Ok(Json(state.supervisor.interrupt(&id)?))
}

async fn list_approvals(State(state): State<Shared>) -> ApiResult<Json<Vec<Approval>>> {
    Ok(Json(state.approvals.all()?))
}

async fn get_approval(
    State(state): State<Shared>,
    Path(id): Path<String>,
) -> ApiResult<Json<Approval>> {
    Ok(Json(state.approvals.get(&id)?))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DecideBody {
    approve: bool,
    #[serde(default)]
    reason: Option<String>,
}

async fn decide_approval(
    State(state): State<Shared>,
    Path(id): Path<String>,
    Json(body): Json<DecideBody>,
) -> ApiResult<Json<Approval>> {
    Ok(Json(state.approvals.decide(
        &id,
        body.approve,
        body.reason.as_deref(),
    )?))
}
