//! Proposal accept and reject routes (ticket 0043).
//!
//! The routes are thin: the opaque proposal id travels in the path, the empty
//! JSON object in the body is validated by the shared JSON body helper, and the
//! accept/reject outcome of the proposal service is mapped onto the standard
//! error shape. The accept applies through the service, so a stale proposal
//! answers `409 conflict` and a midway failure answers `500 partial_apply` with
//! the applied and failing paths.

use axum::{
    Json,
    extract::{Path, State, rejection::JsonRejection},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::error::ApiError;
use crate::proposal::ProposalError;
use crate::state::AppState;

use super::docs::json_body;

/// The empty JSON object both decision routes accept; the decision itself
/// travels in the opaque proposal id of the path.
#[derive(Debug, Deserialize)]
pub(crate) struct DecisionRequest {}

/// The response of a rejected proposal.
#[derive(Debug, Serialize)]
pub(crate) struct RejectResponse {
    /// Whether the proposal was discarded; `true` on a success.
    pub(crate) rejected: bool,
}

/// The response of an accepted proposal: the paths that were applied and the
/// optional PRD feature index refresh warnings.
#[derive(Debug, Serialize)]
pub(crate) struct AcceptResponse {
    /// The paths that were written or deleted, in block order.
    pub(crate) applied: Vec<String>,
    /// The warnings from refreshing the PRD feature index after the apply; the
    /// key is absent when there were none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) warnings: Option<Vec<String>>,
}

/// Handles `POST /api/proposals/{id}/accept`.
///
/// The proposal id is opaque and is never parsed: it is looked up as the string
/// the path captured. The empty JSON body is validated before the service runs,
/// so a malformed body answers with the standard error shape. A stale proposal
/// answers `409 conflict`, an unknown one `404 not_found`, and a midway apply
/// answers `500 partial_apply` carrying the applied and failing paths.
pub(crate) async fn accept(
    State(state): State<AppState>,
    Path(id): Path<String>,
    body: Result<Json<DecisionRequest>, JsonRejection>,
) -> Result<Response, ApiError> {
    let _request: DecisionRequest = json_body(body)?;

    match state.proposals.accept(&id).await {
        Ok(report) => Ok(Json(AcceptResponse {
            applied: report.applied,
            warnings: (!report.warnings.is_empty()).then_some(report.warnings),
        })
        .into_response()),
        Err(ProposalError::NotFound(_)) => Err(ApiError::not_found()),
        Err(ProposalError::Conflict) => Err(ApiError::conflict()),
        Err(ProposalError::InProgress) => Err(ApiError::conflict()),
        Err(ProposalError::Partial {
            applied, failed, ..
        }) => {
            // The error envelope keeps the standard shape, and the partial
            // details sit beside it; the diff UI ticket picks the final shape.
            let body = json!({
                "error": {
                    "code": "partial_apply",
                    "message": "the proposal apply stopped midway",
                },
                "applied": applied,
                "failed": failed,
            });
            Ok((StatusCode::INTERNAL_SERVER_ERROR, Json(body)).into_response())
        }
        Err(error) => Err(ApiError::from(error)),
    }
}

/// Handles `POST /api/proposals/{id}/reject`.
///
/// The proposal id is opaque and is never parsed; an unknown id answers with
/// the standard `404 not_found` error shape.
pub(crate) async fn reject(
    State(state): State<AppState>,
    Path(id): Path<String>,
    body: Result<Json<DecisionRequest>, JsonRejection>,
) -> Result<Response, ApiError> {
    let _request: DecisionRequest = json_body(body)?;

    state.proposals.reject(&id).map_err(ApiError::from)?;

    Ok(Json(RejectResponse { rejected: true }).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_accept_response_serializes_the_applied_paths_and_the_optional_warnings() {
        let response = AcceptResponse {
            applied: vec!["sdlc/features/export.md".to_string()],
            warnings: None,
        };

        let value = serde_json::to_value(&response).expect("the accept response should serialize");
        assert_eq!(value["applied"], json!(["sdlc/features/export.md"]));
        assert!(value.get("warnings").is_none());
    }

    #[test]
    fn the_reject_response_serializes_the_rejection() {
        let response = RejectResponse { rejected: true };

        assert_eq!(
            serde_json::to_value(&response).expect("the reject response should serialize"),
            json!({ "rejected": true })
        );
    }
}
