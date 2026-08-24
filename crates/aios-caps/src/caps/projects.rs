//! The registry, exposed as capabilities.
//!
//! These do not go through a port: the registry *is* AIOS, not a swappable
//! backend. They are here so that the projection in §2 covers everything a
//! client needs, rather than clients reaching for the registry by a side door.

use crate::context::Context;
use crate::registry::{Capability, Effect};
use aios_core::Result;
use aios_types::{NewProject, Project, ProjectSummary};
use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ListInput {
    pub tag: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GetInput {
    /// Slug, id, or path. Defaults to the current directory.
    pub project: Option<String>,
}

pub fn register(items: &mut Vec<Capability>) {
    items.push(Capability::new(
        "projects.list",
        "List registered projects",
        Effect::Read,
        |ctx: &Context, input: ListInput| -> Result<Vec<ProjectSummary>> {
            ctx.registry.list(input.tag.as_deref())
        },
    ));

    items.push(Capability::new(
        "projects.get",
        "Show one registered project in full",
        Effect::Read,
        |ctx: &Context, input: GetInput| -> Result<Project> {
            ctx.project(input.project.as_deref())
        },
    ));

    items.push(Capability::new(
        "projects.add",
        "Register a project directory",
        Effect::Write,
        |ctx: &Context, input: NewProject| -> Result<Project> { ctx.registry.add(input) },
    ));
}
