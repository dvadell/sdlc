//! Document read and save routes (tickets 0018, 0019).

use axum::{
    Json,
    extract::{Path, State, rejection::JsonRejection},
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};

use crate::error::ApiError;
use crate::state::AppState;
use crate::store::layout::DocKind;
use crate::store::{DocEntry, DocPath, Document, Expectation, Tree};

/// The tree payload the sidebar consumes.
#[derive(Debug, Serialize)]
struct TreeResponse {
    /// Whether the `sdlc/` layout directory exists.
    layout_exists: bool,
    /// The documents in deterministic store order.
    docs: Vec<DocEntryResponse>,
}

/// One tree entry, with the path and kind flattened for JSON.
#[derive(Debug, Serialize)]
struct DocEntryResponse {
    /// The validated project-relative path.
    path: String,
    /// The kind group the sidebar renders: core, epic, feature, decision or
    /// ticket.
    kind: &'static str,
    /// The document title.
    title: String,
    /// The front matter status, when present.
    status: Option<String>,
    /// The epic directory of a ticket.
    epic: Option<String>,
    /// The optional feature binding of a ticket.
    feature: Option<String>,
}

/// A document's content and current content hash.
#[derive(Debug, Serialize)]
struct DocResponse {
    /// The validated project-relative path.
    path: String,
    /// The document content, byte-for-byte as UTF-8 text.
    content: String,
    /// Lowercase hex SHA-256 of the content bytes.
    hash: String,
}

/// The body of a save request.
#[derive(Debug, Deserialize)]
pub(crate) struct SaveRequest {
    /// The replacement content.
    content: String,
    /// The hash the client last saw, checked before overwriting.
    expected_hash: String,
}

/// The result of a save.
#[derive(Debug, Serialize)]
pub(crate) struct SaveResponse {
    /// Lowercase hex SHA-256 of the bytes that were written.
    hash: String,
}

impl From<Tree> for TreeResponse {
    fn from(tree: Tree) -> Self {
        Self {
            layout_exists: tree.layout_exists,
            docs: tree.docs.into_iter().map(DocEntryResponse::from).collect(),
        }
    }
}

impl From<DocEntry> for DocEntryResponse {
    fn from(entry: DocEntry) -> Self {
        Self {
            path: entry.path.as_path().to_string_lossy().into_owned(),
            kind: kind_name(entry.kind),
            title: entry.title,
            status: entry.status,
            epic: entry.epic,
            feature: entry.feature,
        }
    }
}

impl From<Document> for DocResponse {
    fn from(document: Document) -> Self {
        Self {
            path: document.path.as_path().to_string_lossy().into_owned(),
            content: document.content,
            hash: document.hash,
        }
    }
}

/// The stable JSON name of a document kind.
fn kind_name(kind: DocKind) -> &'static str {
    match kind {
        DocKind::Core(_) => "core",
        DocKind::Epic => "epic",
        DocKind::Feature => "feature",
        DocKind::Decision => "decision",
        DocKind::Ticket => "ticket",
    }
}

/// Handles `GET /api/tree`.
pub async fn get_tree(State(state): State<AppState>) -> Response {
    no_store(Json(TreeResponse::from(state.store.tree().await)))
}

/// Handles `GET /api/docs/{*path}`.
pub async fn get_doc(
    State(state): State<AppState>,
    Path(raw): Path<String>,
) -> Result<Response, ApiError> {
    let path = parse_captured_path(&raw)?;
    let document = state.store.read(&path).await?;

    Ok(no_store(Json(DocResponse::from(document))))
}

/// Handles `PUT /api/docs/{*path}`.
///
/// The path is parsed first and a blank expected hash is rejected before the
/// store is called, so neither failure touches the disk.
pub(crate) async fn put_doc(
    State(state): State<AppState>,
    Path(raw): Path<String>,
    body: Result<Json<SaveRequest>, JsonRejection>,
) -> Result<Json<SaveResponse>, ApiError> {
    let path = parse_captured_path(&raw)?;
    let SaveRequest {
        content,
        expected_hash,
    } = save_request(body)?;
    ensure_expected_hash(&expected_hash)?;

    let hash = state
        .store
        .write(&path, &content, &Expectation::Existing(expected_hash))
        .await?;

    Ok(Json(SaveResponse { hash }))
}

/// Converts a JSON body rejection into the shared error envelope.
///
/// Axum's own 413 body is plain text, so an oversized save is rebuilt as an
/// [`ApiError`] to keep the API error shape uniform. Other body failures are
/// client errors with a short message.
fn save_request(body: Result<Json<SaveRequest>, JsonRejection>) -> Result<SaveRequest, ApiError> {
    match body {
        Ok(Json(request)) => Ok(request),
        Err(rejection) if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE => {
            Err(ApiError::payload_too_large())
        }
        Err(rejection) => {
            tracing::warn!(status = %rejection.status(), "save request body rejected");
            Err(ApiError::bad_request())
        }
    }
}

/// Rejects a missing or whitespace-only expected hash before any disk access.
fn ensure_expected_hash(expected_hash: &str) -> Result<(), ApiError> {
    if expected_hash.trim().is_empty() {
        Err(ApiError::bad_request())
    } else {
        Ok(())
    }
}

/// Parses a wildcard capture into a [`DocPath`].
///
/// Axum 0.8 normally captures a wildcard without its leading slash, but the
/// exact behaviour has changed between versions, so one optional leading slash
/// is removed before the strict path validation runs.
fn parse_captured_path(raw: &str) -> Result<DocPath, ApiError> {
    let normalized = raw.strip_prefix('/').unwrap_or(raw);

    Ok(DocPath::parse(normalized)?)
}

/// Marks a response as uncacheable, so a browser refresh always re-reads disk.
fn no_store(response: impl IntoResponse) -> Response {
    let mut response = response.into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));

    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_captured_path_with_or_without_a_leading_slash() {
        for raw in ["sdlc/PRD.md", "/sdlc/PRD.md"] {
            let path = parse_captured_path(raw).expect("the path should parse");

            assert_eq!(path.as_path(), std::path::Path::new("sdlc/PRD.md"));
        }
    }

    #[test]
    fn removes_only_one_leading_slash() {
        let error = parse_captured_path("//sdlc/PRD.md").expect_err("two slashes are not a path");

        assert_eq!(error.to_string(), "path is not allowed");
    }

    #[test]
    fn a_rejected_wildcard_capture_is_not_allowed() {
        for raw in ["../Cargo.toml", "/../Cargo.toml", "%2e%2e%2fCargo.toml"] {
            let error = parse_captured_path(raw).expect_err("the traversal should be rejected");

            assert_eq!(error.to_string(), "path is not allowed");
        }
    }

    #[test]
    fn maps_every_document_kind_to_a_stable_name() {
        assert_eq!(kind_name(DocKind::Core(crate::store::CoreDoc::Prd)), "core");
        assert_eq!(kind_name(DocKind::Epic), "epic");
        assert_eq!(kind_name(DocKind::Feature), "feature");
        assert_eq!(kind_name(DocKind::Decision), "decision");
        assert_eq!(kind_name(DocKind::Ticket), "ticket");
    }

    #[test]
    fn rejects_blank_expected_hashes() {
        for hash in ["", " ", "\t\n"] {
            let error = ensure_expected_hash(hash).expect_err("a blank hash must be rejected");

            assert_eq!(error.to_string(), "bad request");
        }

        assert!(ensure_expected_hash("abc").is_ok());
    }
}
