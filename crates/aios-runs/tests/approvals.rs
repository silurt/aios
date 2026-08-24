//! Policy and approval lifecycle.
//!
//! These encode §7.1's claim: because nothing can reliably reach a human, runs
//! must degrade gracefully rather than block forever, and most requests must
//! never become questions at all.

use aios_core::store::DocStore;
use aios_runs::policy::{Policy, Rule, Verdict};
use aios_runs::{Approvals, Request};
use aios_types::{ApprovalState, Decider, RunId};

fn approvals(name: &str) -> Approvals {
    let root = std::env::temp_dir().join(format!("aios-appr-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    Approvals::new(DocStore::new(root))
}

fn request(tool: &str, summary: &str) -> Request {
    Request {
        run_id: RunId("01TESTRUN".into()),
        project: Some("demo".into()),
        tool: tool.into(),
        summary: summary.into(),
        detail: None,
    }
}

#[test]
fn policy_is_first_match_wins_with_an_explicit_default() {
    let policy = Policy::default();

    assert_eq!(policy.decide("Read", "anything").verdict, Verdict::Allow);
    assert_eq!(
        policy.decide("Bash", "git status --short").verdict,
        Verdict::Allow
    );
    assert_eq!(
        policy.decide("Bash", "rm -rf /tmp/x").verdict,
        Verdict::Deny
    );
    // No rule covers this, so the default applies.
    assert_eq!(policy.decide("Edit", "src/main.rs").verdict, Verdict::Ask);
}

#[test]
fn destructive_commands_are_denied_rather_than_asked() {
    // An unattended run must not sit waiting for permission to `rm -rf`, and
    // someone answering a notification at a glance is exactly who would
    // mis-approve one.
    let policy = Policy::default();
    for command in [
        "rm -rf node_modules",
        "git push --force origin main",
        "git reset --hard",
    ] {
        let decision = policy.decide("Bash", command);
        assert_eq!(decision.verdict, Verdict::Deny, "{command}");
        assert!(
            decision.rule.is_some(),
            "a denial must name its rule: {command}"
        );
    }
}

#[test]
fn every_decision_names_the_rule_that_made_it() {
    // "Which rule decided this?" must always have one answer — it is what
    // makes the policy auditable.
    let decision = Policy::default().decide("Bash", "git diff HEAD");
    assert_eq!(decision.rule.as_deref(), Some("allow-git-diff"));

    let defaulted = Policy::default().decide("Write", "somewhere");
    assert!(defaulted.rule.is_none(), "the default is not a rule");
}

#[test]
fn only_unconditional_allows_become_a_harness_allowlist() {
    // A rule with `contains` depends on the specific invocation, which a
    // tool-name allowlist cannot express — promoting one would allow every
    // Bash command because `git status` was allowed.
    let allowed = Policy::default().always_allowed_tools();
    assert!(allowed.contains(&"Read".to_string()));
    assert!(
        !allowed.contains(&"Bash".to_string()),
        "Bash was allowed wholesale: {allowed:?}"
    );
}

#[test]
fn policy_settled_requests_are_still_recorded() {
    // A decision that leaves no trace cannot be audited (§9).
    let store = approvals("recorded");
    let approval = store
        .raise(&Policy::default(), request("Read", "src/lib.rs"))
        .unwrap();

    assert_eq!(approval.state, ApprovalState::Approved);
    assert_eq!(approval.decided_by, Some(Decider::Policy));
    assert_eq!(approval.rule.as_deref(), Some("allow-read"));
    assert_eq!(
        store.get(approval.id.as_str()).unwrap().state,
        ApprovalState::Approved
    );
}

#[test]
fn an_unmatched_request_waits_for_a_human() {
    let store = approvals("pending");
    let approval = store
        .raise(&Policy::default(), request("Edit", "src/main.rs"))
        .unwrap();

    assert_eq!(approval.state, ApprovalState::Pending);
    assert!(approval.decided_by.is_none());
    assert_eq!(store.pending().unwrap().len(), 1);
}

#[test]
fn approving_records_who_decided_and_why() {
    let store = approvals("approve");
    let a = store
        .raise(&Policy::default(), request("Edit", "x"))
        .unwrap();

    let decided = store
        .decide(a.id.as_str(), true, Some("reviewed the diff"))
        .unwrap();
    assert_eq!(decided.state, ApprovalState::Approved);
    assert_eq!(decided.decided_by, Some(Decider::User));
    assert_eq!(decided.reason.as_deref(), Some("reviewed the diff"));
    assert!(store.pending().unwrap().is_empty());
}

#[test]
fn a_settled_approval_cannot_be_flipped() {
    let store = approvals("flip");
    let a = store
        .raise(&Policy::default(), request("Edit", "x"))
        .unwrap();
    store.decide(a.id.as_str(), false, None).unwrap();

    let err = store.decide(a.id.as_str(), true, None).unwrap_err();
    assert!(err.to_string().contains("already"), "{err}");
}

#[test]
fn overdue_requests_expire_and_stop_being_pending() {
    let store = approvals("expire");
    let policy = Policy {
        timeout_secs: 0,
        ..Policy::default()
    };
    let a = store.raise(&policy, request("Edit", "x")).unwrap();
    assert_eq!(a.state, ApprovalState::Pending);

    let expired = store.expire_overdue().unwrap();
    assert_eq!(expired.len(), 1);
    assert_eq!(expired[0].decided_by, Some(Decider::Timeout));
    assert!(store.pending().unwrap().is_empty());
}

#[test]
fn expiry_is_evaluated_on_read_not_by_a_timer() {
    // The daemon may have been stopped for the whole window, so nothing could
    // have fired a timer. Reading must still give the right answer.
    let store = approvals("lazyexpire");
    let policy = Policy {
        timeout_secs: 0,
        ..Policy::default()
    };
    store.raise(&policy, request("Edit", "x")).unwrap();

    // No explicit expire call — `pending` does it.
    assert!(store.pending().unwrap().is_empty());
}

#[test]
fn an_expired_approval_can_still_be_decided_later() {
    // This is the whole point of parking rather than failing: you were away,
    // the run stopped, and your answer when you return is still the answer.
    let store = approvals("latedecision");
    let policy = Policy {
        timeout_secs: 0,
        ..Policy::default()
    };
    let a = store.raise(&policy, request("Edit", "x")).unwrap();
    store.expire_overdue().unwrap();
    assert_eq!(
        store.get(a.id.as_str()).unwrap().state,
        ApprovalState::Expired
    );

    let decided = store
        .decide(a.id.as_str(), true, Some("back at my desk"))
        .unwrap();
    assert_eq!(decided.state, ApprovalState::Approved);
    assert_eq!(decided.decided_by, Some(Decider::User));
}

#[test]
fn outcome_treats_a_lapsed_deadline_as_a_denial() {
    // A gate polling this must not proceed just because nothing formally
    // expired it yet.
    let store = approvals("outcome");
    let policy = Policy {
        timeout_secs: 0,
        ..Policy::default()
    };
    let a = store.raise(&policy, request("Edit", "x")).unwrap();

    let now = time::OffsetDateTime::now_utc() + time::Duration::seconds(1);
    assert_eq!(store.outcome(a.id.as_str(), now).unwrap(), Some(false));
}

#[test]
fn a_custom_policy_can_be_stricter_than_the_default() {
    let policy = Policy {
        rules: vec![Rule {
            name: "deny-everything".into(),
            tool: "*".into(),
            contains: None,
            verdict: Verdict::Deny,
        }],
        default: Verdict::Deny,
        timeout_secs: 60,
    };
    assert_eq!(policy.decide("Read", "anything").verdict, Verdict::Deny);
    assert!(policy.always_allowed_tools().is_empty());
}
