//! The registry, exposed as capabilities.
//!
//! These do not go through a port: the registry *is* AIOS, not a swappable
//! backend. They are here so that the projection in §2 covers everything a
//! client needs, rather than clients reaching for the registry by a side door.

use crate::context::Context;
use crate::registry::{Capability, Effect};
use aios_core::Result;
use aios_types::{ListProjectsInput, NewProject, Project, ProjectRef, ProjectSummary};

pub fn register(items: &mut Vec<Capability>) {
    items.push(Capability::new(
        "projects.list",
        "List registered projects",
        Effect::Read,
        |ctx: &Context, input: ListProjectsInput| -> Result<Vec<ProjectSummary>> {
            ctx.registry.list(input.tag.as_deref())
        },
    ));

    items.push(Capability::new(
        "projects.get",
        "Show one registered project in full",
        Effect::Read,
        |ctx: &Context, input: ProjectRef| -> Result<Project> {
            ctx.project(input.project.as_deref())
        },
    ));

    items.push(Capability::new(
        "projects.add",
        "Register a project directory",
        Effect::Write,
        |ctx: &Context, input: NewProject| -> Result<Project> { ctx.registry.add(input) },
    ));

    items.push(Capability::new(
        "projects.refresh",
        "Re-run detection over a registered project",
        Effect::Write,
        |ctx: &Context, input: ProjectRef| -> Result<Project> {
            let needle = input.project.unwrap_or_else(|| ".".to_string());
            ctx.registry.refresh(&needle)
        },
    ));

    items.push(Capability::new(
        "projects.remove",
        "Remove a project from the registry. Does not touch the directory",
        Effect::Write,
        |ctx: &Context, input: ProjectRef| -> Result<Project> {
            let needle = input
                .project
                .ok_or_else(|| aios_core::Error::Invalid("`project` is required".into()))?;
            ctx.registry.remove(&needle)
        },
    ));
}
