//! What every handler is given.

use aios_caps::{Capabilities, Context, Ports};
use aios_core::Result;
use aios_runs::{Approvals, Policy, Supervisor};
use std::sync::Arc;

pub struct AppState {
    pub capabilities: Capabilities,
    pub context: Context,
    pub supervisor: Supervisor,
    pub approvals: Approvals,
    pub policy: Policy,
}

pub type Shared = Arc<AppState>;

impl AppState {
    /// Assemble from concrete adapters.
    ///
    /// The composition root moved here from the CLI: once the daemon exists it
    /// is the only process that binds ports to adapters, and the CLI reaches
    /// them over the socket like every other client (plan §3.1).
    pub fn new(ports: Ports, policy: Policy) -> Result<Self> {
        let config = aios_core::Config::load()?;
        let registry = aios_core::Registry::open()?;
        Ok(Self {
            capabilities: Capabilities::all(),
            context: Context::new(config, registry, ports),
            supervisor: Supervisor::open(policy.clone())?,
            approvals: Approvals::open()?,
            policy,
        })
    }
}
