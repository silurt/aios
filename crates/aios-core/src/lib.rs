//! AIOS core: the registry, state, and configuration that every surface sits on.
//!
//! Per the tier rule (plan §1.1) nothing here may assume a client exists, and
//! per §3.1 only the `aios daemon *` subcommands are permitted to depend on this
//! crate directly once the API lands in phase 4. Until then the CLI calls it
//! in-process — a known, temporary exception recorded so it does not become
//! permanent by accident.

pub mod config;
pub mod db;
pub mod detect;
pub mod error;
pub mod registry;

pub use config::Config;
pub use error::{Error, Result};
pub use registry::Registry;
