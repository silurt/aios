//! Assembling the capability layer.
//!
//! This is the composition root: the one place that knows which concrete
//! adapter sits behind each port. `aios-caps` deliberately does not, which is
//! what keeps the dependency graph acyclic and makes swapping beads for Linear
//! a change to this file plus one new crate.
//!
//! Phase 0/1 note: the CLI builds this in-process. From phase 4 the daemon owns
//! it and the CLI reaches it over the Unix socket instead (plan §3.1). The
//! commands are written against `Capabilities` rather than the ports directly,
//! so that swap does not reach into command code.

use aios_caps::{Capabilities, Context, Ports};
use anyhow::{Context as _, Result};

pub fn context() -> Result<Context> {
    let config = aios_core::Config::load()?;
    let registry = aios_core::Registry::open()?;
    let ports = Ports {
        issues: Box::new(aios_beads::Beads::new()),
        knowledge: Box::new(aios_obsidian::Vault::new(config.vault.clone())),
        vcs: Box::new(aios_git::Git::new()),
    };
    Ok(Context::new(config, registry, ports))
}

pub fn capabilities() -> Capabilities {
    Capabilities::all()
}

/// The approval policy in force.
///
/// Loaded from `~/.aios/policy.json` when present, so it can be edited by hand
/// like everything else (§16); otherwise the built-in default, which is useful
/// unattended without being reckless.
pub fn policy() -> Result<aios_runs::Policy> {
    let path = aios_core::config::home().join("policy.json");
    match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text)
            .with_context(|| format!("{} is not a valid policy", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(aios_runs::Policy::default()),
        Err(e) => Err(e.into()),
    }
}
