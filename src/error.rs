//! The one error shape returned by API routes and the server startup.

use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;
use std::fmt::Debug;
use std::io;
use thiserror::Error;
use tracing::error;

use crate::proposal::ProposalError;
use crate::store::StoreError;

/// Errors from starting the loopback server.
#[derive(Debug, Error)]
pub enum ServerError {
    /// The configured loopback port is already taken.
    #[error("port {port} is already in use; try --port <other>")]
    AddrInUse { port: u16 },

    /// The listener could not be bound or the server could not be started.
    #[error("failed to start server on 127.0.0.1:{port}: {source}")]
    Io {
        port: u16,
        #[source]
        source: io::Error,
    },

    /// The AI chat client could not be built from the configuration.
    #[error("failed to build the AI chat client: {source}")]
    Chat {
        /// The underlying chat configuration error.
        #[source]
        source: crate::chat::ChatError,
    },
}

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
    /// The paths that were applied before a partial proposal apply failed, in
    /// block order; set only while the error is a partial apply.
    applied: Option<Vec<String>>,
    /// The path of the change that failed during a partial proposal apply.
    failed: Option<String>,
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

    /// Builds the `503 Service Unavailable` error for an unconfigured AI
    /// endpoint.
    ///
    /// The chat UI uses this code to explain the missing setup variables.
    pub fn ai_not_configured() -> Self {
        Self::fixed(
            StatusCode::SERVICE_UNAVAILABLE,
            "ai_not_configured",
            "chat is not configured",
        )
    }

    /// Builds a `413 Payload Too Large` error.
    pub fn payload_too_large() -> Self {
        Self::fixed(
            StatusCode::PAYLOAD_TOO_LARGE,
            "payload_too_large",
            "payload too large",
        )
    }

    /// Builds a `415 Unsupported Media Type` error.
    pub fn unsupported_media_type() -> Self {
        Self::fixed(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "unsupported_media_type",
            "unsupported media type",
        )
    }

    /// Builds the `500 Internal Server Error` error of a proposal accept that
    /// stopped midway, carrying the applied and failing paths.
    ///
    /// The accept already applied some changes, so the response carries the
    /// applied paths and the failing path beside the error envelope; see
    /// [`crate::routes::proposals`].
    pub fn partial_apply(applied: Vec<String>, failed: String) -> Self {
        let mut error = Self::fixed(
            StatusCode::INTERNAL_SERVER_ERROR,
            "partial_apply",
            "the proposal apply stopped midway",
        );
        error.applied = Some(applied);
        error.failed = Some(failed);
        error
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
    ///
    /// The message accepts anything string-convertible (`&str` or an owned
    /// `String` such as a formatted dynamic detail) so every fixed-status error
    /// shares this single construction site.
    fn fixed(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
            applied: None,
            failed: None,
        }
    }

    /// The JSON error envelope: exactly `error.code` and `error.message`.
    fn envelope(&self) -> serde_json::Value {
        json!({ "error": { "code": self.code, "message": self.message } })
    }
}

impl From<StoreError> for ApiError {
    fn from(err: StoreError) -> Self {
        match err {
            StoreError::NotAllowed(_) => Self::fixed(
                StatusCode::BAD_REQUEST,
                "not_allowed",
                "path is not allowed",
            ),
            StoreError::NotFound(_) => {
                Self::fixed(StatusCode::NOT_FOUND, "not_found", "document not found")
            }
            StoreError::AlreadyExists(_) => {
                Self::fixed(StatusCode::CONFLICT, "conflict", "document already exists")
            }
            StoreError::Conflict { .. } => Self::conflict(),
            StoreError::InvalidUtf8(cause) => {
                tracing::warn!(error = %cause, "document is not valid UTF-8");
                Self::fixed(
                    StatusCode::BAD_REQUEST,
                    "invalid_utf8",
                    "document is not valid UTF-8",
                )
            }
            StoreError::InvalidTitle(_) => {
                Self::fixed(StatusCode::BAD_REQUEST, "bad_request", "invalid title")
            }
            StoreError::NumberExhausted { .. } => Self::fixed(
                StatusCode::CONFLICT,
                "conflict",
                "no document numbers remain",
            ),
            StoreError::NumberChange { .. } => Self::fixed(
                StatusCode::BAD_REQUEST,
                "not_allowed",
                "number cannot change",
            ),
            StoreError::EpicHasTickets { tickets } => Self::fixed(
                StatusCode::CONFLICT,
                "conflict",
                crate::store::epic_has_tickets_message(tickets),
            ),
            StoreError::SelfSupersede { supersedes } => {
                tracing::warn!(supersedes, "a decision cannot supersede itself");
                Self::fixed(
                    StatusCode::BAD_REQUEST,
                    "bad_request",
                    "a decision cannot supersede itself",
                )
            }
            StoreError::InvalidSupersedes { supersedes } => {
                tracing::warn!(supersedes, "the supersedes decision number is invalid");
                Self::fixed(
                    StatusCode::BAD_REQUEST,
                    "bad_request",
                    "the supersedes decision number is invalid",
                )
            }
            StoreError::Io(cause) => {
                error!("store io error: {cause:?}");
                Self::fixed(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "io",
                    "internal server error",
                )
            }
        }
    }
}

impl From<ProposalError> for ApiError {
    fn from(error: ProposalError) -> Self {
        match error {
            ProposalError::NotFound(_) => ApiError::not_found(),
            ProposalError::Conflict => ApiError::conflict(),
            ProposalError::InProgress => ApiError::conflict(),
            ProposalError::Partial {
                applied, failed, ..
            } => ApiError::partial_apply(applied, failed),
            // The proposal build refusals are client-input validation failures;
            // the store and diff task failures are internal ones.
            ProposalError::EmptyProposal
            | ProposalError::InvalidPath(_)
            | ProposalError::DuplicatePath(_)
            | ProposalError::MissingTarget(_) => ApiError::bad_request(),
            ProposalError::Store(_) | ProposalError::DiffTask(_) => ApiError::internal(&error),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut body = self.envelope();
        if let Some(applied) = &self.applied {
            body["applied"] = serde_json::json!(applied);
        }
        if let Some(failed) = &self.failed {
            body["failed"] = serde_json::json!(failed);
        }
        (self.status, Json(body)).into_response()
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
    fn payload_too_large_response_matches_the_error_contract() {
        assert_error_contract(
            ApiError::payload_too_large(),
            StatusCode::PAYLOAD_TOO_LARGE,
            "payload_too_large",
            "payload too large",
        );
    }

    #[test]
    fn conflict_error_body_does_not_contain_a_path() {
        let err = ApiError::from(StoreError::Conflict {
            current_hash: "/home/person/project/sdlc/PRD.md".to_string(),
        });

        assert_eq!(err.status, StatusCode::CONFLICT);
        assert_eq!(err.code, "conflict");
        let body = serde_json::to_string(&err.envelope()).expect("error body should serialize");
        assert_eq!(
            body,
            r#"{"error":{"code":"conflict","message":"conflict"}}"#
        );
        assert!(!body.contains("/home/person"));
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
            applied: None,
            failed: None,
        };

        let body = serde_json::to_value(err.envelope()).expect("error body should serialize");
        assert_eq!(
            body,
            json!({ "error": { "code": "bad_request", "message": "quote \" and newline\n" } })
        );
    }

    #[test]
    fn store_errors_map_to_stable_statuses_and_codes() {
        let cases = [
            (
                StoreError::NotAllowed("/secret/path".to_string()),
                StatusCode::BAD_REQUEST,
                "not_allowed",
            ),
            (
                StoreError::NotFound("sdlc/PRD.md".to_string()),
                StatusCode::NOT_FOUND,
                "not_found",
            ),
            (
                StoreError::Conflict {
                    current_hash: "abc".to_string(),
                },
                StatusCode::CONFLICT,
                "conflict",
            ),
            (
                StoreError::InvalidSupersedes { supersedes: 0 },
                StatusCode::BAD_REQUEST,
                "bad_request",
            ),
            (
                StoreError::SelfSupersede { supersedes: 1 },
                StatusCode::BAD_REQUEST,
                "bad_request",
            ),
            (
                StoreError::Io(io::Error::other("disk failed")),
                StatusCode::INTERNAL_SERVER_ERROR,
                "io",
            ),
        ];

        for (store_error, status, code) in cases {
            let api_error = ApiError::from(store_error);

            assert_eq!(api_error.status, status);
            assert_eq!(api_error.code, code);
        }
    }

    #[test]
    fn proposal_errors_map_to_their_statuses_and_codes() {
        let not_found = ApiError::from(ProposalError::NotFound("no-such".to_string()));

        assert_eq!(not_found.status, StatusCode::NOT_FOUND);
        assert_eq!(not_found.code, "not_found");

        let conflict = ApiError::from(ProposalError::Conflict);

        assert_eq!(conflict.status, StatusCode::CONFLICT);
        assert_eq!(conflict.code, "conflict");

        let partial = ApiError::from(ProposalError::Partial {
            applied: vec!["sdlc/PRD.md".to_string()],
            failed: "sdlc/features/export.md".to_string(),
            reason: "the write failed".to_string(),
        });

        assert_eq!(partial.status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(partial.code, "partial_apply");
    }

    #[test]
    fn epic_with_tickets_refusal_carries_the_ticket_count() {
        let error = ApiError::from(StoreError::EpicHasTickets { tickets: 3 });

        assert_eq!(error.status, StatusCode::CONFLICT);
        assert_eq!(error.code, "conflict");
        assert_eq!(error.message, "epic has 3 tickets and cannot be deleted");
    }

    #[test]
    fn epic_with_one_ticket_refusal_singularizes_the_noun() {
        let error = ApiError::from(StoreError::EpicHasTickets { tickets: 1 });

        assert_eq!(error.status, StatusCode::CONFLICT);
        assert_eq!(error.code, "conflict");
        assert_eq!(error.message, "epic has 1 ticket and cannot be deleted");
    }

    #[test]
    fn store_errors_do_not_expose_absolute_paths_or_os_text() {
        for store_error in [
            StoreError::NotAllowed("/home/person/project/sdlc/PRD.md".to_string()),
            StoreError::Io(io::Error::other(
                "cannot open /home/person/project/sdlc/PRD.md",
            )),
        ] {
            let body = serde_json::to_string(&ApiError::from(store_error).envelope())
                .expect("error body should serialize");

            assert!(!body.contains("/home/person"));
            assert!(!body.contains("cannot open"));
        }
    }
}
