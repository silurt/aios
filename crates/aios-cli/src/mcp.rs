//! `aios mcp …` — serving the capability registry over MCP.

use anyhow::Result;
use clap::Subcommand;

#[derive(Subcommand)]
pub enum McpCommand {
    /// Serve over stdio. This is what a harness spawns; not useful by hand.
    Serve,
}

pub fn run(cmd: McpCommand) -> Result<()> {
    match cmd {
        McpCommand::Serve => serve(),
    }
}

fn serve() -> Result<()> {
    // stdout *is* the transport — a stray println here corrupts the protocol
    // stream, which surfaces as an opaque parse failure in the client rather
    // than as anything traceable. Everything diagnostic goes to stderr.
    let context = crate::app::context()?;
    let server = aios_mcp::AiosMcp::new(context);

    // A current-thread runtime: this process exists to shuttle one stdio
    // conversation, and capability work is handed to `spawn_blocking` anyway,
    // so worker threads would idle.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    eprintln!(
        "aios mcp: serving {} capabilities over stdio",
        server.tool_count()
    );
    runtime
        .block_on(server.serve_stdio())
        .map_err(|e| anyhow::anyhow!("mcp server failed: {e}"))
}
