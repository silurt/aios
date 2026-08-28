//! Runs, exposed as capabilities.
//!
//! Like `projects.*`, these do not go through a port — the supervisor *is*
//! AIOS. They are here so an agent or a client reaches runs the same way it
//! reaches everything else, rather than needing bespoke endpoints.

use crate::context::Context;
use crate::registry::{Capability, Effect};
use aios_core::Result;
use aios_types::{Run, RunRef};

pub fn register(items: &mut Vec<Capability>) {
    items.push(Capability::new(
        "runs.interrupt",
        "Stop a running agent",
        Effect::Write,
        |ctx: &Context, input: RunRef| -> Result<Run> { ctx.runs.interrupt(&input.run) },
    ));
}
