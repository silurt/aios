//! API version negotiation (plan §15.4).
//!
//! Clients compile in the `apiVersion` of the spec they were generated from and
//! send it on every request. The daemon compares and answers precisely, because
//! the alternative — a client silently failing to decode a response — is the
//! worst way to discover a mismatch.
//!
//! Skew runs **both ways**, and the direction people forget is the common one:
//! not an old app against a new daemon, but an updated app against a
//! LaunchAgent still running the previous binary because nothing restarted it.

use aios_types::{API_VERSION, MIN_CLIENT_API};
use axum::extract::Request;
use axum::http::{HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

pub const HEADER: &str = "x-aios-api-version";
pub const DEPRECATED_HEADER: &str = "x-aios-api-deprecated";

/// What the daemon thinks of a client's contract version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compatibility {
    /// Same version, or a client that did not declare one.
    Compatible,
    /// Older, but still served. The response is marked deprecated.
    ClientDeprecated,
    /// Older than this build serves.
    ClientTooOld,
    /// Newer than this build understands — usually a stale daemon.
    DaemonTooOld,
}

pub fn classify(client: Option<u32>) -> Compatibility {
    // An absent header is not an error. `curl` and a shell script are
    // legitimate clients, and demanding a version from them would make the API
    // unusable by hand for no safety gain.
    let Some(client) = client else {
        return Compatibility::Compatible;
    };
    if client == API_VERSION {
        Compatibility::Compatible
    } else if client > API_VERSION {
        Compatibility::DaemonTooOld
    } else if client >= MIN_CLIENT_API {
        Compatibility::ClientDeprecated
    } else {
        Compatibility::ClientTooOld
    }
}

/// Reject incompatible clients, and flag merely-old ones.
pub async fn negotiate(request: Request, next: Next) -> Response {
    let client = request
        .headers()
        .get(HEADER)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u32>().ok());

    match classify(client) {
        Compatibility::Compatible => next.run(request).await,

        Compatibility::ClientDeprecated => {
            let mut response = next.run(request).await;
            // A header rather than a failure: it still works, and a client that
            // wants to nag its user can, without anything breaking today.
            response
                .headers_mut()
                .insert(DEPRECATED_HEADER, HeaderValue::from_static("true"));
            response
        }

        // 426 rather than 400: the request was fine, the *protocol* is not, and
        // 426 is the status that means exactly that. The body carries both
        // versions so a client can say which side needs updating rather than
        // showing "something went wrong".
        Compatibility::ClientTooOld => upgrade_required(
            client,
            "this client is older than the daemon serves; update the app",
        ),
        Compatibility::DaemonTooOld => upgrade_required(
            client,
            "the daemon is older than this client; restart it \
             (`aios daemon start`) or reinstall",
        ),
    }
}

fn upgrade_required(client: Option<u32>, message: &str) -> Response {
    (
        StatusCode::UPGRADE_REQUIRED,
        axum::Json(serde_json::json!({
            "kind": "failedPrecondition",
            "message": message,
            "clientApiVersion": client,
            "apiVersion": API_VERSION,
            "minClientApi": MIN_CLIENT_API,
        })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_undeclared_version_is_served() {
        // curl and shell scripts are legitimate clients.
        assert_eq!(classify(None), Compatibility::Compatible);
    }

    #[test]
    fn the_current_version_is_compatible() {
        assert_eq!(classify(Some(API_VERSION)), Compatibility::Compatible);
    }

    #[test]
    fn a_newer_client_means_a_stale_daemon() {
        // The common real case: the app updated, the LaunchAgent did not
        // restart.
        assert_eq!(classify(Some(API_VERSION + 1)), Compatibility::DaemonTooOld);
    }

    #[test]
    fn an_older_but_supported_client_is_deprecated_not_rejected() {
        // Only meaningful once MIN_CLIENT_API < API_VERSION; assert the
        // boundary logic directly so it is right when that day comes.
        assert_eq!(
            classify_with(5, 2, Some(3)),
            Compatibility::ClientDeprecated
        );
    }

    #[test]
    fn a_client_below_the_floor_is_rejected() {
        assert_eq!(classify_with(5, 3, Some(2)), Compatibility::ClientTooOld);
    }

    /// The same rules with explicit bounds, so the boundaries are testable
    /// without waiting for the constants to move.
    fn classify_with(api: u32, floor: u32, client: Option<u32>) -> Compatibility {
        let Some(client) = client else {
            return Compatibility::Compatible;
        };
        if client == api {
            Compatibility::Compatible
        } else if client > api {
            Compatibility::DaemonTooOld
        } else if client >= floor {
            Compatibility::ClientDeprecated
        } else {
            Compatibility::ClientTooOld
        }
    }
}
