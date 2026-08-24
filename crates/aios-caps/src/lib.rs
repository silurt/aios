//! The capability layer: port traits, and the registry that projects them onto
//! every surface (plan §2).

pub mod caps;
pub mod context;
pub mod ports;
pub mod registry;

pub use context::{Context, Ports};
pub use ports::{IssueTracker, Knowledge, Vcs};
pub use registry::{Capabilities, Capability, Effect};
