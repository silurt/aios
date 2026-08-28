//! The daemon's HTTP surface.
//!
//! One `axum` router served over every transport (plan §3.2). Today that is a
//! Unix socket, which is all the CLI and a same-machine app need; TLS over the
//! LAN and the tailnet come in phase 6. Handlers never learn which door a
//! request came through.

pub mod error;
pub mod routes;
pub mod state;
pub mod version;

pub use state::{AppState, Shared};

use aios_core::Result;
use std::path::Path;

/// Serve on a Unix domain socket until interrupted.
///
/// The socket is created at mode 0600 and a stale one is removed first — a
/// daemon that died without cleaning up would otherwise make every restart
/// fail with "address already in use", which is a miserable thing to debug.
pub async fn serve_uds(state: Shared, path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if path.exists() {
        std::fs::remove_file(path)?;
    }

    // Unix socket paths are capped by `sockaddr_un.sun_path` — 104 bytes on
    // macOS, 108 on Linux. Bind fails with "path must be shorter than SUN_LEN",
    // which says nothing about what to do, so check first and say it.
    const SUN_PATH_MAX: usize = 100;
    let rendered = path.display().to_string();
    if rendered.len() > SUN_PATH_MAX {
        return Err(aios_core::Error::Invalid(format!(
            "socket path is {} bytes; the OS limit is about {SUN_PATH_MAX}. \
             Set AIOS_HOME to something shorter: {rendered}",
            rendered.len()
        )));
    }

    let listener = tokio::net::UnixListener::bind(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }

    let app = routes::router(state);
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown())
        .await
        .map_err(aios_core::Error::Io)?;

    // Leaving the socket behind would be the stale-socket problem we just
    // worked around, inflicted on the next start.
    let _ = std::fs::remove_file(path);
    Ok(())
}

async fn shutdown() {
    let ctrl_c = async { tokio::signal::ctrl_c().await.ok() };
    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .ok()?
            .recv()
            .await
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<Option<()>>();

    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }
}
