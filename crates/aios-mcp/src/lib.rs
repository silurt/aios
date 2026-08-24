//! The MCP server — the capability registry, projected onto tools.
//!
//! This is §2's claim made concrete: no tool is defined here. `list_tools`
//! enumerates `Capabilities::all()` and `call_tool` dispatches straight into it,
//! so Claude Code and Codex see exactly the operations the CLI exposes, with the
//! same schemas and the same handlers. Adding a capability adds a tool
//! everywhere at once; there is no second registration to forget.
//!
//! Transport is stdio for now (plan §3.2 also wants streamable HTTP, which
//! arrives with the daemon in phase 4).

use aios_caps::{Capabilities, Context};
use rmcp::handler::server::ServerHandler;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
    ListToolsResult, PaginatedRequestParams, ProtocolVersion, ServerCapabilities, ServerInfo, Tool,
    ToolAnnotations,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, ServiceExt};
use std::sync::Arc;

/// Tools are prefixed so they are unmistakable in a client that has several
/// servers connected. Capability names are already dotted (`issues.ready`), but
/// MCP tool names are a flat namespace shared across every server the client
/// has loaded.
const TOOL_PREFIX: &str = "aios_";

fn tool_name(capability: &str) -> String {
    format!("{TOOL_PREFIX}{}", capability.replace('.', "_"))
}

fn capability_name(tool: &str) -> String {
    tool.strip_prefix(TOOL_PREFIX)
        .unwrap_or(tool)
        .replacen('_', ".", 1)
}

pub struct AiosMcp {
    /// `Arc` because each tool call is handed to `spawn_blocking`: capability
    /// handlers shell out to `bd` and `git` and touch the filesystem, so running
    /// them on a tokio worker would stall the reactor and, with a slow `bd`
    /// invocation, stall the transport with it.
    inner: Arc<Inner>,
}

struct Inner {
    capabilities: Capabilities,
    context: Context,
}

impl AiosMcp {
    pub fn new(context: Context) -> Self {
        Self {
            inner: Arc::new(Inner {
                capabilities: Capabilities::all(),
                context,
            }),
        }
    }

    /// Serve over stdio until the client disconnects.
    ///
    /// Nothing may be written to stdout except protocol frames — stdout *is*
    /// the transport. Diagnostics go to stderr, which is also where the CLI
    /// sends its logging for this subcommand.
    pub async fn serve_stdio(self) -> Result<(), Box<dyn std::error::Error>> {
        let running = self.serve(rmcp::transport::stdio()).await?;
        running.waiting().await?;
        Ok(())
    }

    /// How many tools this server will advertise. Used for the startup line on
    /// stderr, which is the only signal a human gets that it came up.
    pub fn tool_count(&self) -> usize {
        self.inner.capabilities.len()
    }

    fn tools(&self) -> Vec<Tool> {
        self.inner
            .capabilities
            .iter()
            .map(|c| {
                let schema = c.input_schema.as_object().cloned().unwrap_or_default();
                // `Effect` earns its keep here: a client that distinguishes
                // read-only tools can auto-approve them, and one that does not
                // still shows the hint to the model.
                let annotations = ToolAnnotations::new()
                    .read_only(!c.effect.is_write())
                    .destructive(false);
                Tool::new(tool_name(c.name), c.summary, Arc::new(schema)).annotate(annotations)
            })
            .collect()
    }
}

impl ServerHandler for AiosMcp {
    fn get_info(&self) -> ServerInfo {
        // Both types are #[non_exhaustive], so they are built by mutation
        // rather than a struct literal — rmcp adds protocol fields between
        // releases and this is what keeps that additive.
        let mut info = ServerInfo::default();
        info.protocol_version = ProtocolVersion::LATEST;
        info.capabilities = ServerCapabilities::builder().enable_tools().build();
        info.server_info = {
            let mut implementation = Implementation::default();
            implementation.name = "aios".into();
            implementation.version = env!("CARGO_PKG_VERSION").into();
            implementation
        };
        // Instructions are the server's one chance to orient a model before it
        // starts guessing, so they say what exists and where to start.
        info.instructions = Some(
            "AIOS exposes this machine's registered projects, their issue trackers \
             (beads), a shared Obsidian knowledge base, and git status. Every tool \
             takes an optional `project` argument — a slug, id, or path — which \
             defaults to the current working directory. Start with \
             aios_projects_list to see what exists, and aios_issues_ready to find \
             work that is unblocked."
                .into(),
        );
        info
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        // The registry is small and static for a given build, so there is
        // nothing to paginate.
        Ok(ListToolsResult {
            tools: self.tools(),
            ..Default::default()
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let name = capability_name(&request.name);
        let arguments = serde_json::Value::Object(request.arguments.unwrap_or_default());
        let inner = Arc::clone(&self.inner);

        let outcome = tokio::task::spawn_blocking(move || {
            inner.capabilities.call(&inner.context, &name, arguments)
        })
        .await
        .map_err(|e| ErrorData::internal_error(format!("tool task failed: {e}"), None))?;

        match outcome {
            Ok(value) => {
                let text = serde_json::to_string_pretty(&value)
                    .unwrap_or_else(|e| format!("could not serialize result: {e}"));
                Ok(CallToolResult::success(vec![ContentBlock::text(text)]).into())
            }
            // A failed capability is a *tool* error, not a protocol error: the
            // model should see what went wrong and be able to correct itself,
            // which it cannot do if the transport rejects the call instead.
            Err(e) => Ok(CallToolResult::error(vec![ContentBlock::text(e.to_string())]).into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_names_round_trip_through_the_prefix() {
        for capability in ["issues.ready", "kb.search", "projects.list", "vcs.status"] {
            let tool = tool_name(capability);
            assert!(tool.starts_with(TOOL_PREFIX));
            assert!(
                !tool.contains('.'),
                "MCP tool names should not contain dots"
            );
            assert_eq!(capability_name(&tool), capability);
        }
    }

    #[test]
    fn capability_name_restores_only_the_first_separator() {
        // `issues.status` must not become `issues.status` -> `issues_status`
        // -> `issues.status` by accident if a capability ever has two dots; the
        // replacen(1) is deliberate and this pins it.
        assert_eq!(capability_name("aios_kb_write"), "kb.write");
        assert_eq!(capability_name("kb_write"), "kb.write");
    }
}
