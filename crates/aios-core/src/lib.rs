//! AIOS core: the registry, state, and configuration that every surface sits on.
//!
//! Per the tier rule (plan §1.1) nothing here may assume a client exists, and
//! per §3.1 only the `aios daemon *` subcommands are permitted to depend on this
//! crate directly once the API lands in phase 4. Until then the CLI calls it
//! in-process — a known, temporary exception recorded so it does not become
//! permanent by accident.

pub mod config;
pub mod detect;
pub mod error;
pub mod managed;
pub mod registry;
pub mod store;

pub use config::Config;

/// Today's date as `YYYY-MM-DD`, in local time.
///
/// Local rather than UTC deliberately: this names daily notes and inbox files,
/// and a note filed at 1am should carry the date the person filing it would
/// say out loud.
pub fn today() -> String {
    let now = time::OffsetDateTime::now_local().unwrap_or_else(|_| time::OffsetDateTime::now_utc());
    format!(
        "{:04}-{:02}-{:02}",
        now.year(),
        now.month() as u8,
        now.day()
    )
}
pub use error::{Error, Result};
pub use registry::Registry;
