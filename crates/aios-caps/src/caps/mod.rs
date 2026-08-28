//! Capability definitions, grouped by port.
//!
//! Every input type here is a plain `serde` struct so it can be deserialized
//! from an MCP tool call, a REST body, or a CLI `--input` argument
//! interchangeably. `project: Option<String>` appears throughout and always
//! means the same thing: a slug, id, or path, defaulting to the current
//! directory.

pub mod issues;
pub mod knowledge;
pub mod projects;
pub mod runs;
pub mod vcs;
