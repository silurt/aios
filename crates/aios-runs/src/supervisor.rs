//! The run supervisor: spawn a harness, normalize its output, persist it.
//!
//! Everything a run produces lands in two places: a `Run` document holding its
//! current state, and an append-only JSONL event log holding its transcript.
//! The log is written as events arrive rather than at the end, so a client can
//! follow a run live and a crashed run still leaves everything that happened
//! before the crash (§16).

use crate::policy::Policy;
use aios_caps::ports::{Harness, RunSpec};
use aios_core::store::{AppendLog, DocStore, Sequenced};
use aios_core::{Error, Result};
use aios_types::{HarnessId, Run, RunEvent, RunId, RunStatus};
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use time::OffsetDateTime;

pub const COLLECTION: &str = "runs";

/// What to run.
pub struct StartRun {
    pub harness: Box<dyn Harness>,
    pub prompt: String,
    pub cwd: PathBuf,
    pub project: Option<String>,
    pub model: Option<String>,
}

pub struct Supervisor {
    store: DocStore,
    home: PathBuf,
    policy: Policy,
}

impl Supervisor {
    pub fn open(policy: Policy) -> Result<Self> {
        let home = aios_core::config::ensure_home()?;
        Ok(Self {
            store: DocStore::new(&home),
            home,
            policy,
        })
    }

    pub fn at(home: impl Into<PathBuf>, policy: Policy) -> Self {
        let home = home.into();
        Self {
            store: DocStore::new(&home),
            home,
            policy,
        }
    }

    /// The event log for a run.
    pub fn log(&self, id: &RunId) -> AppendLog {
        AppendLog::new(
            self.home
                .join("runs")
                .join(id.as_str())
                .join("events.jsonl"),
        )
    }

    pub fn get(&self, id: &str) -> Result<Run> {
        self.store
            .get::<Run>(COLLECTION, id)?
            .ok_or_else(|| Error::ProjectNotFound(format!("run {id}")))
    }

    /// All runs, newest first — ULID ids make filename order creation order.
    pub fn all(&self) -> Result<Vec<Run>> {
        let mut runs = self.store.list::<Run>(COLLECTION)?;
        runs.reverse();
        Ok(runs)
    }

    /// Replay a run's transcript from `since`, the §13.2 cursor.
    pub fn events(&self, id: &str, since: u64, limit: usize) -> Result<Vec<Sequenced<RunEvent>>> {
        self.log(&RunId(id.to_string())).read_since(since, limit)
    }

    /// Run a harness to completion, streaming events as they arrive.
    ///
    /// `on_event` is called for every normalized event *after* it is durably
    /// logged, so a caller rendering live output can never show something that
    /// would be missing from a replay.
    pub fn run(
        &self,
        start: StartRun,
        mut on_event: impl FnMut(&Sequenced<RunEvent>),
    ) -> Result<Run> {
        let spec = RunSpec {
            prompt: start.prompt.clone(),
            cwd: start.cwd.clone(),
            model: start.model.clone(),
            // Anything policy allows unconditionally is handed to the harness
            // as an allowlist, so those calls never become approval requests at
            // all — cheaper than deciding them, and it keeps the transcript
            // about the work (§7.1).
            allowed_tools: self.policy.always_allowed_tools(),
            disallowed_tools: Vec::new(),
        };

        let mut run = Run {
            id: RunId(ulid::Ulid::from_datetime(std::time::SystemTime::now()).to_string()),
            harness: start.harness.id(),
            project: start.project.clone(),
            cwd: start.cwd.display().to_string(),
            prompt: start.prompt.clone(),
            status: RunStatus::Running,
            session_ref: None,
            model: start.model.clone(),
            last_seq: 0,
            exit_code: None,
            error: None,
            cost_usd: None,
            turns: None,
            started_at: OffsetDateTime::now_utc(),
            ended_at: None,
        };
        // Persist before spawning: a run that dies immediately must still be
        // visible, not a process nobody has a record of.
        self.store.put(COLLECTION, run.id.as_str(), &run)?;

        let log = self.log(&run.id);
        let args = start.harness.command(&spec);

        let mut child = Command::new(start.harness.binary())
            .args(&args)
            .current_dir(&start.cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| match e.kind() {
                std::io::ErrorKind::NotFound => Error::ToolMissing {
                    tool: start.harness.binary().to_string(),
                },
                _ => Error::Io(e),
            })?;

        let stdout = child.stdout.take().expect("stdout was piped");
        let mut record = |event: RunEvent, run: &mut Run| -> Result<()> {
            let at = OffsetDateTime::now_utc()
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap_or_default();
            let seq = log.append(&event, &at)?;
            run.last_seq = seq;
            on_event(&Sequenced {
                seq,
                at,
                v: aios_core::store::log::RECORD_VERSION,
                data: event,
            });
            Ok(())
        };

        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            for event in start.harness.translate(&line) {
                // Interesting fields are lifted onto the run document as they
                // appear, so `run show` never has to replay a whole transcript
                // to answer "which session? how much did it cost?".
                match &event {
                    RunEvent::Started {
                        session_ref, model, ..
                    } => {
                        run.session_ref = session_ref.clone();
                        if run.model.is_none() {
                            run.model = model.clone();
                        }
                        // Persist immediately. The approval gate correlates a
                        // permission request to its run by session id, and the
                        // gate fires *during* the run — leaving this until the
                        // run ends orphans every approval it raises.
                        self.store.put(COLLECTION, run.id.as_str(), &run)?;
                    }
                    RunEvent::Finished {
                        cost_usd, turns, ..
                    } => {
                        run.cost_usd = *cost_usd;
                        run.turns = *turns;
                    }
                    _ => {}
                }
                record(event, &mut run)?;
            }
        }

        let status = child.wait()?;
        // stderr is read after the stream closes rather than concurrently:
        // harnesses use it for diagnostics, not for a second event stream, and
        // it only matters when something went wrong.
        let stderr = child
            .stderr
            .take()
            .map(|s| std::io::read_to_string(s).unwrap_or_default())
            .unwrap_or_default();

        run.exit_code = status.code();
        run.ended_at = Some(OffsetDateTime::now_utc());
        run.status = if status.success() {
            RunStatus::Succeeded
        } else {
            RunStatus::Failed
        };

        if !status.success() {
            let detail = stderr.trim();
            let error = if detail.is_empty() {
                format!("{} exited with {}", start.harness.binary(), status)
            } else {
                detail.lines().rev().take(5).collect::<Vec<_>>().join(" | ")
            };
            run.error = Some(error.clone());
            record(RunEvent::Failed { error }, &mut run)?;
        }

        self.store.put(COLLECTION, run.id.as_str(), &run)?;
        Ok(run)
    }

    /// Park a run at an unanswered gate.
    ///
    /// Not a failure: the workspace and transcript stay, and the run can be
    /// resumed once a decision arrives. Without push (§13.3) this is the
    /// ordinary outcome of an unattended run, not an error path.
    pub fn park(&self, id: &str, reason: &str) -> Result<Run> {
        self.store.with_lock(|| {
            let mut run = self.get(id)?;
            run.status = RunStatus::Parked;
            run.error = Some(reason.to_string());
            self.store.put(COLLECTION, run.id.as_str(), &run)?;
            Ok(run)
        })
    }

    /// Whether a parked run can be continued through the harness, or has to be
    /// started over. Answering this needs the harness, so it is exposed rather
    /// than assumed by callers.
    pub fn is_resumable(&self, run: &Run, harness: &dyn Harness) -> bool {
        run.session_ref.is_some()
            && harness
                .resume_command(
                    &RunSpec {
                        prompt: run.prompt.clone(),
                        cwd: PathBuf::from(&run.cwd),
                        model: run.model.clone(),
                        allowed_tools: Vec::new(),
                        disallowed_tools: Vec::new(),
                    },
                    run.session_ref.as_deref().unwrap_or_default(),
                )
                .is_some()
    }
}

/// Pick a harness by id.
pub fn harness_for(id: HarnessId) -> Box<dyn Harness> {
    match id {
        HarnessId::Claude => Box::new(aios_claude::Claude::new()),
        HarnessId::Codex => Box::new(aios_codex::Codex::new()),
    }
}
