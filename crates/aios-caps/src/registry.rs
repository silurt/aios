//! The capability registry — the mechanism §2 describes.
//!
//! A capability is registered once: a name, a summary, a read/write
//! classification, and a handler with typed input and output. From that single
//! registration everything else is derived:
//!
//! - the **CLI** calls it by name with JSON (`aios cap call`),
//! - the **MCP server** (phase 2) enumerates the same list into tools,
//! - the **REST API** (phase 4) mounts the same handlers,
//! - and because input and output are ordinary `serde` types from
//!   `aios-types`, the OpenAPI spec and every generated client follow (§15).
//!
//! The typed handler is wrapped into an erased `Value -> Value` closure at
//! registration time. That is what lets one list serve both a statically typed
//! Rust caller and a dynamically dispatched MCP tool call, without writing the
//! operation twice.

use crate::context::Context;
use aios_core::{Error, Result};
use schemars::JsonSchema;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

/// Whether a capability observes or changes the world.
///
/// This is not decoration. It decides which MCP annotation a tool gets, which
/// operations a read-only agent profile may call, and — from phase 3 — what
/// needs an approval before it runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    Read,
    Write,
}

impl Effect {
    pub fn is_write(&self) -> bool {
        matches!(self, Effect::Write)
    }
}

type Handler = Box<dyn Fn(&Context, Value) -> Result<Value> + Send + Sync>;

pub struct Capability {
    pub name: &'static str,
    pub summary: &'static str,
    pub effect: Effect,
    /// JSON Schema for the input, derived from the input type.
    ///
    /// This is what an MCP client shows the model as a tool's `inputSchema`, so
    /// the doc comments on the input struct are load-bearing prompt text rather
    /// than internal notes.
    pub input_schema: Value,
    handler: Handler,
}

impl Capability {
    /// Register a typed handler.
    ///
    /// The `I: DeserializeOwned` / `O: Serialize` bounds are what make the
    /// erasure safe: a caller can only reach the handler through JSON that
    /// deserializes into the declared input type, so a malformed MCP tool call
    /// fails at the boundary with a decode error rather than inside the port.
    pub fn new<I, O, F>(
        name: &'static str,
        summary: &'static str,
        effect: Effect,
        handler: F,
    ) -> Self
    where
        I: DeserializeOwned + JsonSchema,
        O: Serialize,
        F: Fn(&Context, I) -> Result<O> + Send + Sync + 'static,
    {
        Self {
            name,
            summary,
            effect,
            input_schema: serde_json::to_value(schemars::schema_for!(I))
                .expect("a derived JSON Schema is always serializable"),
            handler: Box::new(move |ctx, raw| {
                let input: I = serde_json::from_value(raw)
                    .map_err(|e| Error::Invalid(format!("invalid input for {name}: {e}")))?;
                let output = handler(ctx, input)?;
                Ok(serde_json::to_value(output)?)
            }),
        }
    }

    pub fn call(&self, ctx: &Context, input: Value) -> Result<Value> {
        (self.handler)(ctx, input)
    }
}

/// Every capability AIOS exposes, in one place.
pub struct Capabilities {
    items: Vec<Capability>,
}

impl Capabilities {
    /// Build the full set. Registration is explicit rather than collected by a
    /// linker-section crate: the list is greppable, ordering is controlled, and
    /// nothing appears in the API because a macro ran somewhere.
    pub fn all() -> Self {
        let mut items = Vec::new();
        crate::caps::issues::register(&mut items);
        crate::caps::knowledge::register(&mut items);
        crate::caps::vcs::register(&mut items);
        crate::caps::projects::register(&mut items);
        items.sort_by_key(|c| c.name);
        Self { items }
    }

    pub fn iter(&self) -> impl Iterator<Item = &Capability> {
        self.items.iter()
    }

    pub fn get(&self, name: &str) -> Result<&Capability> {
        self.items
            .iter()
            .find(|c| c.name == name)
            .ok_or_else(|| Error::CapabilityNotFound(name.to_string()))
    }

    pub fn call(&self, ctx: &Context, name: &str, input: Value) -> Result<Value> {
        self.get(name)?.call(ctx, input)
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}
