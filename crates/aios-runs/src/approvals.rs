//! Approvals: raising them, deciding them, and expiring them into a park.

use crate::policy::Policy;
use aios_core::store::DocStore;
use aios_core::{Error, Result};
use aios_types::{Approval, ApprovalId, ApprovalState, Decider, RunId, Timestamp};
use time::OffsetDateTime;

pub const COLLECTION: &str = "approvals";

pub struct Approvals {
    store: DocStore,
}

/// What a harness is asking permission to do.
#[derive(Debug, Clone)]
pub struct Request {
    pub run_id: RunId,
    pub project: Option<String>,
    pub tool: String,
    pub summary: String,
    pub detail: Option<String>,
}

impl Approvals {
    pub fn new(store: DocStore) -> Self {
        Self { store }
    }

    pub fn open() -> Result<Self> {
        Ok(Self::new(DocStore::new(aios_core::config::ensure_home()?)))
    }

    /// Raise a request, applying policy first.
    ///
    /// An approval is written even when policy settles it immediately. The
    /// record of *what an agent was allowed to do and why* is the point — a
    /// decision that leaves no trace cannot be audited, and §9 says every run
    /// is journaled with its tool trace.
    pub fn raise(&self, policy: &Policy, request: Request) -> Result<Approval> {
        let now = OffsetDateTime::now_utc();
        let decision = policy.decide(&request.tool, &request.summary);
        let settled = decision.resolves_to();

        let approval = Approval {
            id: ApprovalId(ulid::Ulid::from_datetime(std::time::SystemTime::now()).to_string()),
            run_id: request.run_id,
            project: request.project,
            tool: request.tool,
            summary: request.summary,
            detail: request.detail,
            state: settled.map(|(s, _)| s).unwrap_or(ApprovalState::Pending),
            decided_by: settled.map(|(_, d)| d),
            rule: decision.rule,
            reason: None,
            requested_at: now,
            expires_at: now + time::Duration::seconds(policy.timeout_secs as i64),
            decided_at: settled.map(|_| now),
        };
        self.put(&approval)?;
        Ok(approval)
    }

    /// A decision already made for this exact request in this run.
    ///
    /// Without this, deciding an expired approval would achieve nothing: the
    /// resumed run hits the same gate, a *new* approval is raised, and the
    /// answer you gave is ignored. It also stops a run re-asking the same
    /// question every time the model retries a tool.
    ///
    /// Scoped to one run and an exact tool+summary match, deliberately. A
    /// decision is about *this* action in *this* run — carrying it across runs
    /// would quietly widen permission far beyond what anyone agreed to.
    pub fn find_decided(
        &self,
        run_id: &RunId,
        tool: &str,
        summary: &str,
    ) -> Result<Option<Approval>> {
        Ok(self.all()?.into_iter().find(|a| {
            &a.run_id == run_id
                && a.tool == tool
                && a.summary == summary
                && matches!(a.state, ApprovalState::Approved | ApprovalState::Denied)
        }))
    }

    pub fn get(&self, id: &str) -> Result<Approval> {
        self.store
            .get::<Approval>(COLLECTION, id)?
            .ok_or_else(|| Error::ProjectNotFound(format!("approval {id}")))
    }

    /// All approvals, newest first.
    ///
    /// Ids are ULIDs, so filename order is creation order and reversing it is
    /// enough — no timestamp comparison needed.
    pub fn all(&self) -> Result<Vec<Approval>> {
        let mut all = self.store.list::<Approval>(COLLECTION)?;
        all.reverse();
        Ok(all)
    }

    /// Approvals still waiting on a human, expiring any that ran out of time
    /// first so a caller never sees a stale `Pending`.
    pub fn pending(&self) -> Result<Vec<Approval>> {
        self.expire_overdue()?;
        Ok(self
            .all()?
            .into_iter()
            .filter(Approval::is_pending)
            .collect())
    }

    /// Record a human decision.
    ///
    /// An expired approval can still be decided. That is the whole point of
    /// parking rather than failing: you were away, the run stopped, and your
    /// answer when you return is still the answer.
    pub fn decide(&self, id: &str, approve: bool, reason: Option<&str>) -> Result<Approval> {
        self.store.with_lock(|| {
            let mut approval = self.get(id)?;
            if matches!(
                approval.state,
                ApprovalState::Approved | ApprovalState::Denied
            ) {
                return Err(Error::Invalid(format!(
                    "{id} was already {} by {:?}",
                    if approval.state == ApprovalState::Approved {
                        "approved"
                    } else {
                        "denied"
                    },
                    approval.decided_by.unwrap_or(Decider::User)
                )));
            }
            approval.state = if approve {
                ApprovalState::Approved
            } else {
                ApprovalState::Denied
            };
            approval.decided_by = Some(Decider::User);
            approval.reason = reason.map(str::to_owned);
            approval.decided_at = Some(OffsetDateTime::now_utc());
            self.put(&approval)?;
            Ok(approval)
        })
    }

    /// Expire anything past its deadline. Returns what was expired.
    ///
    /// Expiry is evaluated on read rather than by a timer, so it is correct
    /// even when nothing was running to fire the timer — the daemon may have
    /// been stopped for the entire window.
    pub fn expire_overdue(&self) -> Result<Vec<Approval>> {
        let now = OffsetDateTime::now_utc();
        let mut expired = Vec::new();
        for mut approval in self.all()? {
            if approval.is_pending() && approval.expires_at <= now {
                approval.state = ApprovalState::Expired;
                approval.decided_by = Some(Decider::Timeout);
                approval.decided_at = Some(now);
                self.put(&approval)?;
                expired.push(approval);
            }
        }
        Ok(expired)
    }

    /// Whether an approval has been settled, and how — the question the gate
    /// polls (see `crate::gate`).
    pub fn outcome(&self, id: &str, now: Timestamp) -> Result<Option<bool>> {
        let approval = self.get(id)?;
        Ok(match approval.state {
            ApprovalState::Approved => Some(true),
            ApprovalState::Denied => Some(false),
            ApprovalState::Expired => Some(false),
            ApprovalState::Pending if approval.expires_at <= now => Some(false),
            ApprovalState::Pending => None,
        })
    }

    fn put(&self, approval: &Approval) -> Result<()> {
        self.store.put(COLLECTION, approval.id.as_str(), approval)
    }
}
