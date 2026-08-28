//! Runs and approvals: the supervisor, and the policy that keeps it usable
//! when nobody is watching.

pub mod approvals;
pub mod policy;
pub mod supervisor;

pub use approvals::{Approvals, Request};
pub use policy::{Policy, Rule, Verdict};
pub use supervisor::Supervisor;

/// Adapter letting capabilities reach the supervisor without `aios-caps`
/// depending on this crate.
pub struct SupervisorControl(pub std::sync::Arc<Supervisor>);

impl aios_caps::RunControl for SupervisorControl {
    fn interrupt(&self, id: &str) -> aios_core::Result<aios_types::Run> {
        self.0.interrupt(id)
    }
}
