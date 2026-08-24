//! Approval policy — deciding what never has to be asked.
//!
//! Plan §7.1: the best approval is the one never raised. Because there is no
//! push (§13.3), the daemon cannot rely on reaching a human, so a design that
//! asks about everything would stall on the first `ls`. Policy is what makes
//! unattended runs useful rather than theoretical.
//!
//! Rules are ordered and first-match-wins, with an explicit default. That is
//! deliberately the same model as a firewall: people already know how to read
//! it, and "which rule decided this?" always has one answer — recorded on the
//! approval as `rule`.

use aios_types::{ApprovalState, Decider};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Verdict {
    Allow,
    Deny,
    /// Raise an approval and wait for a human.
    Ask,
}

/// One rule. Matching is intentionally simple — tool name plus an optional
/// substring of the request — because a rule nobody can read is a rule nobody
/// can audit, and this decides whether an agent may touch a repository.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rule {
    /// Human-readable, and recorded on every approval it settles.
    pub name: String,
    /// Tool name, or `*` for any.
    pub tool: String,
    /// Substring that must appear in the request summary. `None` matches any.
    #[serde(default)]
    pub contains: Option<String>,
    pub verdict: Verdict,
}

impl Rule {
    fn matches(&self, tool: &str, summary: &str) -> bool {
        let tool_ok = self.tool == "*" || self.tool.eq_ignore_ascii_case(tool);
        let text_ok = match &self.contains {
            Some(needle) => summary.to_lowercase().contains(&needle.to_lowercase()),
            None => true,
        };
        tool_ok && text_ok
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Policy {
    pub rules: Vec<Rule>,
    /// What to do when no rule matches.
    pub default: Verdict,
    /// How long a raised approval waits before expiring, in seconds.
    pub timeout_secs: u64,
}

impl Default for Policy {
    /// A default that is useful unattended without being reckless.
    ///
    /// Reads are allowed, obviously-destructive shell commands are denied
    /// outright rather than asked about — an unattended run should not be able
    /// to sit waiting for permission to `rm -rf`, and a human answering a
    /// notification at a glance is exactly who mis-approves one — and
    /// everything else asks.
    fn default() -> Self {
        let rule = |name: &str, tool: &str, contains: Option<&str>, verdict| Rule {
            name: name.to_string(),
            tool: tool.to_string(),
            contains: contains.map(str::to_owned),
            verdict,
        };
        Self {
            rules: vec![
                rule(
                    "deny-recursive-delete",
                    "Bash",
                    Some("rm -rf"),
                    Verdict::Deny,
                ),
                rule(
                    "deny-force-push",
                    "Bash",
                    Some("push --force"),
                    Verdict::Deny,
                ),
                rule(
                    "deny-history-rewrite",
                    "Bash",
                    Some("reset --hard"),
                    Verdict::Deny,
                ),
                rule("allow-read", "Read", None, Verdict::Allow),
                rule("allow-grep", "Grep", None, Verdict::Allow),
                rule("allow-glob", "Glob", None, Verdict::Allow),
                rule(
                    "allow-git-status",
                    "Bash",
                    Some("git status"),
                    Verdict::Allow,
                ),
                rule("allow-git-diff", "Bash", Some("git diff"), Verdict::Allow),
                rule("allow-git-log", "Bash", Some("git log"), Verdict::Allow),
                rule("allow-aios", "Bash", Some("aios "), Verdict::Allow),
            ],
            default: Verdict::Ask,
            timeout_secs: 15 * 60,
        }
    }
}

/// Why a request was decided the way it was.
#[derive(Debug, Clone, PartialEq)]
pub struct Decision {
    pub verdict: Verdict,
    /// Name of the matching rule, or `None` when the default applied.
    pub rule: Option<String>,
}

impl Decision {
    /// The approval state this decision implies, or `None` when a human is
    /// still needed.
    pub fn resolves_to(&self) -> Option<(ApprovalState, Decider)> {
        match self.verdict {
            Verdict::Allow => Some((ApprovalState::Approved, Decider::Policy)),
            Verdict::Deny => Some((ApprovalState::Denied, Decider::Policy)),
            Verdict::Ask => None,
        }
    }
}

impl Policy {
    /// Decide a request. First match wins.
    pub fn decide(&self, tool: &str, summary: &str) -> Decision {
        match self.rules.iter().find(|r| r.matches(tool, summary)) {
            Some(rule) => Decision {
                verdict: rule.verdict,
                rule: Some(rule.name.clone()),
            },
            None => Decision {
                verdict: self.default,
                rule: None,
            },
        }
    }

    /// Tool names this policy allows outright.
    ///
    /// Passed to the harness as its allowlist so those calls never become
    /// requests at all — cheaper than deciding them, and it keeps the transcript
    /// about the work. Only unconditional rules qualify: a rule with `contains`
    /// depends on the specific invocation, which a tool-name allowlist cannot
    /// express.
    pub fn always_allowed_tools(&self) -> Vec<String> {
        self.rules
            .iter()
            .filter(|r| r.verdict == Verdict::Allow && r.contains.is_none() && r.tool != "*")
            .map(|r| r.tool.clone())
            .collect()
    }
}
