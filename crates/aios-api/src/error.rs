//! Turning core errors into HTTP responses.

use aios_core::Error;
use aios_types::{ApiError, ErrorKind};
use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

/// A failure on its way out over HTTP.
///
/// Every failing response is a typed [`ApiError`], so a client branches on
/// `kind` rather than parsing prose or guessing from a status code (§15).
pub struct ApiFailure(pub ApiError);

impl From<Error> for ApiFailure {
    fn from(e: Error) -> Self {
        ApiFailure(ApiError::from(&e))
    }
}

impl IntoResponse for ApiFailure {
    fn into_response(self) -> Response {
        let status = match self.0.kind {
            ErrorKind::NotFound => StatusCode::NOT_FOUND,
            ErrorKind::AlreadyExists => StatusCode::CONFLICT,
            ErrorKind::InvalidArgument => StatusCode::BAD_REQUEST,
            // 412 rather than 400: the request was well-formed, the machine was
            // not in a state to serve it — a missing vault, no tracker in this
            // project. A client can retry after fixing the precondition.
            ErrorKind::FailedPrecondition => StatusCode::PRECONDITION_FAILED,
            ErrorKind::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        };
        (status, Json(self.0)).into_response()
    }
}

pub type ApiResult<T> = std::result::Result<T, ApiFailure>;
