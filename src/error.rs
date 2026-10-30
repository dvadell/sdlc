//! The one error shape returned by API routes.

use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;
use std::fmt::Debug;
use tracing::error;

/// The error response returned by every API route.
///
/// `ApiError` is the only type that knows about HTTP: it owns the status code
/// and the stable machine-readable code, so handlers never choose them.
#[derive(Debug, Clone)]
pub struct ApiError {
    /// HTTP status the response should carry.
    status: StatusCode,
    /// Stable, snake_case machine-readable error code.
    code: &'static str,
    /// Short human-readable message safe to expose to the client.
    message: String,
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ApiError {}

impl ApiError {
    /// Builds a `400 Bad Request` error.
    pub fn bad_request() -> Self {
        Self::fixed(StatusCode::BAD_REQUEST, "bad_request", "bad request")
    }

    /// Builds a `404 Not Found` error.
    pub fn not_found() -> Self {
        Self::fixed(StatusCode::NOT_FOUND, "not_found", "not found")
    }

    /// Builds a `409 Conflict` error.
    pub fn conflict() -> Self {
        Self::fixed(StatusCode::CONFLICT, "conflict", "conflict")
    }

    /// Builds a `403 Forbidden` error.
    pub fn forbidden() -> Self {
        Self::fixed(StatusCode::FORBIDDEN, "forbidden", "forbidden")
    }

    /// Builds a `415 Unsupported Media Type` error.
    pub fn unsupported_media_type() -> Self {
        Self::fixed(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "unsupported_media_type",
            "unsupported media type",
        )
    }

    /// Builds a `500 Internal Server Error`, logging the cause for debugging.
    ///
    /// The cause is logged in full but never serialized into the response:
    /// paths and upstream payloads are internal details.
    pub fn internal(cause: impl Debug) -> Self {
        error!("internal server error: {:?}", cause);
        Self::fixed(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            "internal server error",
        )
    }

    /// Wraps a fixed status, code and client-safe message into an error.
    fn fixed(status: StatusCode, code: &'static str, message: &str) -> Self {
        Self {
            status,
            code,
            message: message.to_string(),
        }
    }

    /// The JSON error envelope: exactly `error.code` and `error.message`.
    fn envelope(&self) -> serde_json::Value {
        json!({ "error": { "code": self.code, "message": self.message } })
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(self.envelope())).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::header;
    use serde_json::json;
    use std::fs;
    use std::sync::Mutex;
    use tempfile::NamedTempFile;
    use tracing::Level;
    use tracing_subscriber::fmt;

    fn assert_error_contract(err: ApiError, status: StatusCode, code: &'static str, message: &str) {
        assert_eq!(err.status, status);
        assert_eq!(err.code, code);
        assert_eq!(err.message, message);

        let body = serde_json::to_value(err.envelope()).expect("error body should serialize");
        assert_eq!(
            body,
            json!({ "error": { "code": code, "message": message } })
        );
    }

    #[test]
    fn bad_request_response_matches_the_error_contract() {
        assert_error_contract(
            ApiError::bad_request(),
            StatusCode::BAD_REQUEST,
            "bad_request",
            "bad request",
        );
    }

    #[test]
    fn not_found_response_matches_the_error_contract() {
        assert_error_contract(
            ApiError::not_found(),
            StatusCode::NOT_FOUND,
            "not_found",
            "not found",
        );
    }

    #[test]
    fn conflict_response_matches_the_error_contract() {
        assert_error_contract(
            ApiError::conflict(),
            StatusCode::CONFLICT,
            "conflict",
            "conflict",
        );
    }

    #[test]
    fn forbidden_response_matches_the_error_contract() {
        assert_error_contract(
            ApiError::forbidden(),
            StatusCode::FORBIDDEN,
            "forbidden",
            "forbidden",
        );
    }

    #[test]
    fn unsupported_media_type_response_matches_the_error_contract() {
        assert_error_contract(
            ApiError::unsupported_media_type(),
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "unsupported_media_type",
            "unsupported media type",
        );
    }

    #[test]
    fn into_response_sets_the_status_and_json_content_type() {
        let response = ApiError::not_found().into_response();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok());
        assert_eq!(content_type, Some("application/json"));
    }

    #[test]
    fn internal_error_response_hides_the_cause() {
        let cause = "secret /home/me/file";
        let err = ApiError::internal(cause);

        assert_eq!(err.status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(err.code, "internal");
        assert_eq!(err.message, "internal server error");

        let body = serde_json::to_value(err.envelope()).expect("error body should serialize");
        assert!(!body.to_string().contains(cause));
    }

    #[test]
    fn internal_error_logs_the_cause() {
        let cause = "secret /home/me/file";
        let log_file = NamedTempFile::new().expect("temp log file");
        let (file, path) = log_file.keep().expect("keep returns the file and its path");
        let subscriber = fmt()
            .with_max_level(Level::DEBUG)
            .with_writer(Mutex::new(file))
            .finish();
        tracing::subscriber::with_default(subscriber, || ApiError::internal(cause));
        let logged = fs::read_to_string(&path).expect("read log");
        assert!(logged.contains(cause));
    }

    #[test]
    fn error_message_survives_json_escaping() {
        let err = ApiError {
            status: StatusCode::BAD_REQUEST,
            code: "bad_request",
            message: "quote \" and newline\n".to_string(),
        };

        let body = serde_json::to_value(err.envelope()).expect("error body should serialize");
        assert_eq!(
            body,
            json!({ "error": { "code": "bad_request", "message": "quote \" and newline\n" } })
        );
    }
}
