//! Document read, save and lifecycle routes (tickets 0018–0020).

use axum::{
    Json,
    extract::{Path, State, rejection::JsonRejection},
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::error::ApiError;
use crate::state::AppState;
use crate::store::layout::{self, DocKind};
use crate::store::{DocEntry, DocPath, Document, Expectation, StoreError, Tree};

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
    /// The optional decision supersedes pointer as a decision number; `None`
    /// for kinds that carry no supersedes.
    supersedes: Option<u32>,
    /// The epic directory of a ticket.
    epic: Option<String>,
    /// The optional feature binding of a ticket.
    feature: Option<String>,
    /// The ticket dependencies of a ticket, as ticket numbers; empty for kinds
    /// that carry no dependency list.
    depends_on: Vec<u32>,
    /// Whether the ticket's epic directory has no description, so a ticket
    /// under such a manually broken directory is orphaned but still listed
    /// (ticket 0048).
    orphaned: bool,
    /// Whether the ticket named a feature that no longer resolves; it does not
    /// affect the epic grouping (ticket 0048).
    feature_orphaned: bool,
}

/// One advisory warning about the PRD (ticket 0057): the PRD grew past the
/// configured size limit, it carries a fenced code block, or a heading names
/// content that belongs in the architecture, conventions or a feature spec.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum PrdWarning {
    /// The PRD content is larger than the configured size limit (ticket 0057).
    #[serde(rename = "prd_size")]
    Size {
        /// The byte length of the PRD content.
        bytes: u64,
        /// The configured maximum size in bytes.
        limit: u64,
    },
    /// The PRD content carries at least one fenced code block (ticket 0057).
    #[serde(rename = "code_block")]
    CodeBlock {
        /// How many fenced code blocks the PRD content carries.
        count: usize,
    },
    /// A PRD heading names content that belongs in another document (ticket
    /// 0057).
    #[serde(rename = "stack_heading")]
    StackHeading {
        /// The heading text that matched.
        heading: String,
    },
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
    /// The advisory warnings of the PRD (ticket 0057); empty for every other
    /// document, because only the PRD carries them.
    warnings: Vec<PrdWarning>,
}

impl DocResponse {
    /// Builds the read response of one document with its advisory warnings.
    fn from_document(document: Document, limit: u64) -> Self {
        let warnings = document_warnings(&document, limit);
        Self {
            path: document.path.as_path().to_string_lossy().into_owned(),
            content: document.content,
            hash: document.hash,
            warnings,
        }
    }
}

/// The advisory warnings of a document read (ticket 0057): only the PRD carries
/// them, and only while its content trips the size limit or the content
/// heuristics; every other document answers with an empty list.
fn document_warnings(document: &Document, limit: u64) -> Vec<PrdWarning> {
    if document.path.as_path() != std::path::Path::new("sdlc/PRD.md") {
        // The size and content warnings belong to the PRD alone (ticket 0057).
        return Vec::new();
    }
    prd_warnings(&document.content, limit)
}

/// The advisory warnings of the PRD content (ticket 0057).
///
/// The warnings are advisory only: the caller never turns them into a failure,
/// so an over-limit PRD can still be saved (ticket 0057).
pub(crate) fn prd_warnings(content: &str, limit: u64) -> Vec<PrdWarning> {
    let mut warnings = Vec::new();
    let bytes = content.len() as u64;
    if bytes > limit {
        warnings.push(PrdWarning::Size { bytes, limit });
    }
    let code_blocks = fenced_code_block_count(content);
    if code_blocks > 0 {
        warnings.push(PrdWarning::CodeBlock { count: code_blocks });
    }
    for heading in stack_headings(content) {
        warnings.push(PrdWarning::StackHeading { heading });
    }
    warnings
}

/// Counts the fenced code blocks of a markdown content (ticket 0057).
///
/// Paired, unpaired and indented fences all count; the open state of a fence is
/// tracked, so a closing marker never opens another block and the marker lines of
/// a block are never read as headings.
fn fenced_code_block_count(content: &str) -> usize {
    let mut count = 0;
    let mut open = false;
    for line in content.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if is_fence_line(line) {
            if open {
                open = false;
            } else {
                open = true;
                count += 1;
            }
        }
    }
    count
}

/// Whether a markdown line opens or closes a fenced code block (ticket 0057).
fn is_fence_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("```") || trimmed.starts_with("~~~")
}

/// The headings of a markdown content whose text contains a stack-ish token,
/// in document order, with the heading text (ticket 0057).
fn stack_headings(content: &str) -> Vec<String> {
    const STACK_TOKENS: [&str; 5] = [
        "dependencies",
        "libraries",
        "stack",
        "tooling",
        "code style",
    ];
    let mut headings = Vec::new();
    let mut open = false;
    for line in content.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if is_fence_line(line) {
            // A fence marker is never a heading, whatever it contains (ticket 0057).
            open = !open;
            continue;
        }
        if open {
            continue;
        }
        if let Some(heading) = atx_heading_text(line) {
            let lower = heading.to_lowercase();
            if STACK_TOKENS.iter().any(|token| lower.contains(*token)) {
                headings.push(heading);
            }
        }
    }
    headings
}

/// The text of an ATX heading line, without its markers (ticket 0057).
fn atx_heading_text(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    let hashes = trimmed.chars().take_while(|c| *c == '#').count();
    if hashes == 0 || hashes > 6 {
        return None;
    }
    let text = trimmed[hashes..]
        .trim()
        .trim_end_matches('#')
        .trim()
        .to_string();
    if text.is_empty() {
        return None;
    }
    Some(text)
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
    /// Warnings from refreshing the PRD feature index after a feature save, so
    /// the person knows the index may be stale; absent for every other document
    /// and when nothing went wrong.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    warnings: Option<Vec<String>>,
}

/// The body of a create request.
#[derive(Debug, Deserialize)]
pub(crate) struct CreateRequest {
    /// The document kind: `epic`, `feature`, `decision` or `ticket`.
    kind: String,
    /// The title rendered into the new document.
    title: String,
    /// The target epic directory for a ticket; ticket-only, so a create of
    /// another kind carrying one is refused.
    #[serde(default)]
    epic: Option<String>,
    /// The optional feature slug for a ticket; ticket-only, so a create of
    /// another kind carrying one is refused.
    #[serde(default)]
    feature: Option<String>,
    /// The status of a decision: `proposed`, `accepted` or `superseded`; the
    /// default is `proposed` (ticket 0031).
    ///
    /// A blank value is treated as absent, so the default applies.
    #[serde(default)]
    status: Option<String>,
    /// The optional decision number a decision supersedes (ticket 0031).
    #[serde(default)]
    supersedes: Option<u32>,
}

/// A created document's path and content hash.
#[derive(Debug, Serialize)]
pub(crate) struct CreateResponse {
    /// The validated project-relative path of the new document.
    path: String,
    /// Lowercase hex SHA-256 of the content that was written.
    hash: String,
    /// Warnings from creating the document: the PRD feature index refresh
    /// outcome for a created feature, or the dangling-supersedes notice for a
    /// created decision; absent for every other kind.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    warnings: Option<Vec<String>>,
}

/// The body of a move request.
#[derive(Debug, Deserialize)]
pub(crate) struct MoveRequest {
    /// The existing document path.
    from: String,
    /// The destination document path.
    to: String,
}

/// The result of a move.
#[derive(Debug, Serialize)]
pub(crate) struct MoveResponse {
    /// The validated project-relative path after the move.
    path: String,
    /// Warnings from refreshing the PRD feature index after a renamed feature,
    /// so the person knows the index may be stale; absent when no feature moved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    warnings: Option<Vec<String>>,
}

/// The result of a delete.
#[derive(Debug, Serialize)]
pub(crate) struct DeleteResponse {
    /// Tickets left pointing at a deleted feature; only present for a feature.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    orphaned_tickets: Option<usize>,
    /// Warnings from refreshing the PRD feature index after a deleted feature,
    /// so the person knows the index may be stale; absent for other kinds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    warnings: Option<Vec<String>>,
}

/// The empty JSON object accepted by the scaffold route.
#[derive(Debug, Deserialize)]
pub(crate) struct ScaffoldRequest {}

/// The files created and skipped by a scaffold run.
#[derive(Debug, Serialize)]
pub(crate) struct ScaffoldResponse {
    /// Starter files that did not exist and were created.
    created: Vec<String>,
    /// Starter files that already existed and were skipped.
    skipped: Vec<String>,
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
            supersedes: entry.supersedes,
            epic: entry.epic,
            feature: entry.feature,
            depends_on: entry.depends_on,
            orphaned: entry.orphaned,
            feature_orphaned: entry.feature_orphaned,
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

    Ok(no_store(Json(DocResponse::from_document(
        document,
        state.config.prd_max_bytes(),
    ))))
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
    } = json_body(body)?;
    ensure_expected_hash(&expected_hash)?;

    let hash = state
        .store
        .write(&path, &content, &Expectation::Existing(expected_hash))
        .await?;

    let warnings = feature_refresh_warnings(&state, &[&path]).await;

    Ok(Json(SaveResponse { hash, warnings }))
}

/// Handles `POST /api/docs`.
///
/// The kind and any path-shaped references are validated before the store is
/// called. A ticket needs an epic directory; its feature binding is optional. A
/// kind-specific field that the kind does not carry is refused, so a status,
/// epic or feature that does not belong to the kind fails fast instead of being
/// silently ignored.
pub(crate) async fn create_doc(
    State(state): State<AppState>,
    body: Result<Json<CreateRequest>, JsonRejection>,
) -> Result<(StatusCode, Json<CreateResponse>), ApiError> {
    let request = json_body(body)?;
    let kind = parse_kind(&request.kind)?;
    let epic = non_blank(request.epic.as_deref());
    let feature = non_blank(request.feature.as_deref());
    let status = match kind {
        DocKind::Decision => Some(parse_decision_status(&request.status)?),
        _ => {
            // The decision status vocabulary is decision-only; a status sent
            // for another kind is a misplaced field and is refused instead of
            // being silently ignored.
            if non_blank(request.status.as_deref()).is_some() {
                return Err(ApiError::bad_request());
            }
            None
        }
    };

    // The epic directory and the feature binding belong to tickets; carrying
    // them for another kind is a misplaced field and is refused instead of
    // being silently ignored.
    if kind != DocKind::Ticket
        && (non_blank(request.epic.as_deref()).is_some()
            || non_blank(request.feature.as_deref()).is_some())
    {
        return Err(ApiError::bad_request());
    }

    // The supersedes pointer belongs to decisions; carrying it for another
    // kind is a misplaced field and is refused instead of being silently
    // ignored.
    if kind != DocKind::Decision && request.supersedes.is_some() {
        return Err(ApiError::bad_request());
    }

    if kind == DocKind::Ticket {
        let epic = epic.ok_or_else(ApiError::bad_request)?;
        ensure_epic_reference(epic)?;
        if let Some(feature) = feature {
            ensure_feature_reference(feature)?;
        }
    }

    let (created, create_warnings) = if kind == DocKind::Decision {
        let (created, warnings) = state
            .store
            .create_decision(
                &request.title,
                status.unwrap_or(crate::store::frontmatter::DecisionStatus::Proposed),
                request.supersedes,
            )
            .await
            .map_err(|error| map_create_error(kind, false, error))?;
        (created, warnings)
    } else {
        let created = state
            .store
            .create(kind, &request.title, epic, feature)
            .await
            .map_err(|error| map_create_error(kind, feature.is_some(), error))?;
        (created, Vec::new())
    };
    let document = state.store.read(&created).await?;

    // The refresh eligibility is derived from the created path, so a feature
    // created through the API keeps the PRD feature index in sync; decisions
    // carry their own create warnings instead (ticket 0031).
    let warnings = if kind == DocKind::Decision {
        (!create_warnings.is_empty()).then_some(create_warnings)
    } else {
        feature_refresh_warnings(&state, &[&created]).await
    };

    Ok((
        StatusCode::CREATED,
        Json(CreateResponse {
            path: path_string(&created),
            hash: document.hash,
            warnings,
        }),
    ))
}

/// Handles `POST /api/docs/move`.
///
/// Both paths become a [`DocPath`] before the store sees them, so a traversal
/// or a missing source/destination never reaches the filesystem.
pub(crate) async fn move_doc(
    State(state): State<AppState>,
    body: Result<Json<MoveRequest>, JsonRejection>,
) -> Result<Json<MoveResponse>, ApiError> {
    let request = json_body(body)?;
    let from = DocPath::parse(&request.from)?;
    let to = DocPath::parse(&request.to)?;
    let moved = state.store.rename(&from, &to).await?;

    // The refresh eligibility is derived from both the source and the
    // destination, so a rename that ever crosses kinds still refreshes the PRD
    // feature index whenever a feature is involved.
    let warnings = feature_refresh_warnings(&state, &[&from, &to]).await;

    Ok(Json(MoveResponse {
        path: path_string(&moved),
        warnings,
    }))
}

/// Handles `DELETE /api/docs/{*path}`.
pub(crate) async fn delete_doc(
    State(state): State<AppState>,
    Path(raw): Path<String>,
) -> Result<Json<DeleteResponse>, ApiError> {
    let path = parse_captured_path(&raw)?;
    let report = state.store.delete(&path).await?;
    let orphaned_tickets =
        (layout::kind(path.as_path()) == Some(DocKind::Feature)).then_some(report.orphaned_tickets);

    let warnings = feature_refresh_warnings(&state, &[&path]).await;

    Ok(Json(DeleteResponse {
        orphaned_tickets,
        warnings,
    }))
}

/// Handles `POST /api/scaffold`.
pub(crate) async fn scaffold(
    State(state): State<AppState>,
    body: Result<Json<ScaffoldRequest>, JsonRejection>,
) -> Result<Json<ScaffoldResponse>, ApiError> {
    let _request: ScaffoldRequest = json_body(body)?;
    let report = state.store.scaffold().await?;

    Ok(Json(ScaffoldResponse {
        created: report.created,
        skipped: report.skipped,
    }))
}

/// Converts a JSON body rejection into the shared error envelope.
///
/// Axum's own 413 body is plain text, so an oversized request is rebuilt as an
/// [`ApiError`] to keep the API error shape uniform. Other body failures are
/// client errors with a short message.
pub(crate) fn json_body<T: DeserializeOwned>(
    body: Result<Json<T>, JsonRejection>,
) -> Result<T, ApiError> {
    match body {
        Ok(Json(request)) => Ok(request),
        Err(rejection) if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE => {
            Err(ApiError::payload_too_large())
        }
        Err(rejection) => {
            tracing::warn!(status = %rejection.status(), "request body rejected");
            Err(ApiError::bad_request())
        }
    }
}

/// Parses the stable JSON name of a creatable document kind.
fn parse_kind(raw: &str) -> Result<DocKind, ApiError> {
    match raw {
        "epic" => Ok(DocKind::Epic),
        "feature" => Ok(DocKind::Feature),
        "decision" => Ok(DocKind::Decision),
        "ticket" => Ok(DocKind::Ticket),
        _ => Err(ApiError::bad_request()),
    }
}

/// Parses the status of a decision create request, defaulting to `proposed`
/// when the field is absent or blank and rejecting any value outside the
/// `proposed | accepted | superseded` vocabulary before the store writes the
/// new decision (ticket 0031).
fn parse_decision_status(
    raw: &Option<String>,
) -> Result<crate::store::frontmatter::DecisionStatus, ApiError> {
    let raw = raw
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("proposed");
    crate::store::frontmatter::DecisionStatus::parse(raw).ok_or_else(ApiError::bad_request)
}

/// The trimmed non-blank value of an optional reference field.
fn non_blank(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

/// Validates an epic directory reference syntactically, before the store runs.
fn ensure_epic_reference(epic: &str) -> Result<(), ApiError> {
    let path = DocPath::parse(&format!("sdlc/epics/{epic}/description.md"))?;
    if layout::kind(path.as_path()) == Some(DocKind::Epic) {
        Ok(())
    } else {
        Err(ApiError::bad_request())
    }
}

/// Validates a feature slug reference syntactically, before the store runs.
fn ensure_feature_reference(feature: &str) -> Result<(), ApiError> {
    let path = DocPath::parse(&format!("sdlc/features/{feature}.md"))?;
    if layout::kind(path.as_path()) == Some(DocKind::Feature) {
        Ok(())
    } else {
        Err(ApiError::bad_request())
    }
}

/// Maps a create failure to its HTTP status.
///
/// A missing feature binding is invalid input (400), while a missing epic is a
/// missing document (404); both are reported by the store as `NotFound`, so a
/// ticket carrying a feature maps that case to 400.
fn map_create_error(kind: DocKind, feature_given: bool, error: StoreError) -> ApiError {
    if kind == DocKind::Ticket && feature_given && matches!(&error, StoreError::NotFound(_)) {
        ApiError::bad_request()
    } else {
        ApiError::from(error)
    }
}

/// The validated path as the JSON string clients round-trip.
fn path_string(path: &DocPath) -> String {
    path.as_path().to_string_lossy().into_owned()
}

/// Rejects a missing or whitespace-only expected hash before any disk access.
fn ensure_expected_hash(expected_hash: &str) -> Result<(), ApiError> {
    if expected_hash.trim().is_empty() {
        Err(ApiError::bad_request())
    } else {
        Ok(())
    }
}

/// Refreshes the PRD feature index after a feature change and collects the
/// warnings for the response; every other document leaves the PRD alone, so
/// editing a non-feature document never touches it. Returns the refresh
/// warnings, or `None` when no refresh applied or nothing went wrong. The
/// refresh runs whenever any of the given paths is a feature, so callers pass
/// every path a mutation may have touched.
async fn feature_refresh_warnings(state: &AppState, paths: &[&DocPath]) -> Option<Vec<String>> {
    if paths
        .iter()
        .any(|path| layout::kind(path.as_path()) == Some(DocKind::Feature))
    {
        let warnings = state.store.refresh_feature_index().await;
        (!warnings.is_empty()).then_some(warnings)
    } else {
        None
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
    use crate::config::{CliArgs, Config};
    use serde_json::json;

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

    #[test]
    fn save_response_serializes_the_optional_warnings_array() {
        let response = SaveResponse {
            hash: "abc".to_string(),
            warnings: Some(vec!["the PRD feature index was not refreshed".to_string()]),
        };

        let value = serde_json::to_value(&response).expect("the save response should serialize");
        assert_eq!(value["hash"], json!("abc"));
        assert_eq!(
            value["warnings"],
            json!(["the PRD feature index was not refreshed"])
        );
    }

    #[test]
    fn save_response_omits_warnings_when_there_are_none() {
        let response = SaveResponse {
            hash: "abc".to_string(),
            warnings: None,
        };

        let value = serde_json::to_value(&response).expect("the save response should serialize");

        assert!(value.get("warnings").is_none());
    }

    fn test_app_state() -> (AppState, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = Config::from_parts(
            CliArgs {
                dir: dir.path().to_path_buf(),
                port: 0,
                open: false,
            },
            |_| None,
        )
        .expect("the test config should build");

        (
            AppState::new(config, 0).expect("the state should build"),
            dir,
        )
    }

    #[tokio::test]
    async fn refresh_runs_when_a_rename_co_involves_a_feature() {
        let (state, _dir) = test_app_state();

        let feature = DocPath::parse("sdlc/features/export.md").expect("valid feature");
        let decision = DocPath::parse("sdlc/decisions/0001-use-axum.md").expect("valid decision");

        // A rename of a feature refreshes the PRD feature index, whether the
        // source or the destination is the feature.
        assert!(
            feature_refresh_warnings(&state, &[&feature, &decision])
                .await
                .is_some(),
            "a rename whose source is a feature must refresh the index"
        );
        assert!(
            feature_refresh_warnings(&state, &[&decision, &feature])
                .await
                .is_some(),
            "a rename whose destination is a feature must refresh the index"
        );
    }

    #[tokio::test]
    async fn refresh_skips_documents_that_are_not_features() {
        let (state, _dir) = test_app_state();

        let decision = DocPath::parse("sdlc/decisions/0001-use-axum.md").expect("valid decision");
        let core = DocPath::parse("sdlc/PRD.md").expect("valid core document");

        assert!(
            feature_refresh_warnings(&state, &[&decision, &core])
                .await
                .is_none(),
            "a rename without a feature must not refresh the index"
        );
    }

    // The PRD warnings of the document response (ticket 0057).

    #[test]
    fn prd_warnings_are_empty_at_and_below_the_limit() {
        let content = "# Product requirements\n";
        assert!(prd_warnings(content, 12000).is_empty());

        // The limit itself does not warn; only a strictly larger content does
        // (ticket 0057).
        let at_limit = "x".repeat(10);
        assert!(prd_warnings(&at_limit, 10).is_empty());

        let over_limit = format!("{at_limit}x");
        let warnings = prd_warnings(&over_limit, 10);
        assert_eq!(warnings.len(), 1);
        assert_eq!(
            warnings[0],
            PrdWarning::Size {
                bytes: over_limit.len() as u64,
                limit: 10
            }
        );
    }

    #[test]
    fn prd_size_warning_counts_bytes_not_chars() {
        // A multi-byte heading is longer in bytes than in characters, so the size
        // warning must measure bytes and not characters (ticket 0057).
        let content = format!("# Product requirements\n\n## {}\n", "é".repeat(30));
        let bytes = content.len() as u64;
        let char_count = content.chars().count() as u64;
        assert!(bytes > char_count, "the fixture must be multi-byte");

        let warnings = prd_warnings(&content, char_count);
        assert_eq!(warnings.len(), 1);
        assert_eq!(
            warnings[0],
            PrdWarning::Size {
                bytes,
                limit: char_count
            }
        );
    }

    #[test]
    fn prd_warnings_count_fenced_code_blocks_and_not_inline_code() {
        let clean = "# Product requirements\n\nSome `inline code`.\n";
        assert!(prd_warnings(clean, 12000).is_empty());

        let paired = "# PRD\n\n```rust\nfn main() {}\n```\n";
        assert_eq!(prd_warnings(paired, 12000).len(), 1);
        assert_eq!(
            prd_warnings(paired, 12000)[0],
            PrdWarning::CodeBlock { count: 1 }
        );

        let indented = "# PRD\n\n  ```rust\n  fn main() {}\n  ```\n";
        assert_eq!(
            prd_warnings(indented, 12000)[0],
            PrdWarning::CodeBlock { count: 1 }
        );

        let unpaired = "# PRD\n\n```rust\nfn main() {}\n";
        assert_eq!(
            prd_warnings(unpaired, 12000)[0],
            PrdWarning::CodeBlock { count: 1 }
        );

        let multiple = "# PRD\n\n```rust\na\n```\n\ntext\n\n```bash\nb\n```\n";
        assert_eq!(
            prd_warnings(multiple, 12000)[0],
            PrdWarning::CodeBlock { count: 2 }
        );
    }

    #[test]
    fn prd_warnings_match_stack_headings_case_insensitively() {
        let content = "# Product requirements\n\n## Dependencies\n\nBody.\n";
        let warnings = prd_warnings(content, 12000);
        assert_eq!(warnings.len(), 1);
        assert_eq!(
            warnings[0],
            PrdWarning::StackHeading {
                heading: "Dependencies".to_string()
            }
        );

        let stacked = "# Product requirements\n\n## STACK\n";
        let warnings = prd_warnings(stacked, 12000);
        assert_eq!(
            warnings[0],
            PrdWarning::StackHeading {
                heading: "STACK".to_string()
            }
        );

        let tooling = "# Product requirements\n\n### libraries and tooling\n";
        let warnings = prd_warnings(tooling, 12000);
        assert_eq!(
            warnings[0],
            PrdWarning::StackHeading {
                heading: "libraries and tooling".to_string()
            }
        );
    }

    #[test]
    fn prd_warnings_ignore_headings_inside_code_fences() {
        let content = "# Product requirements\n\n```\n## Stack\n```\n\n## Overview\n";
        // The fenced block itself warns, but the heading inside the fence does
        // not (ticket 0057).
        let warnings = prd_warnings(content, 12000);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0], PrdWarning::CodeBlock { count: 1 });
    }

    #[test]
    fn document_warnings_are_computed_for_the_prd_only() {
        let prd = Document {
            path: DocPath::parse("sdlc/PRD.md").expect("valid PRD"),
            content: "x".repeat(20_000),
            hash: "abc".to_string(),
        };
        let architecture = Document {
            path: DocPath::parse("sdlc/architecture.md").expect("valid architecture"),
            content: "x".repeat(20_000),
            hash: "abc".to_string(),
        };

        let prd_warnings = document_warnings(&prd, 10);
        assert_eq!(prd_warnings.len(), 1);
        assert_eq!(
            prd_warnings[0],
            PrdWarning::Size {
                bytes: 20_000,
                limit: 10
            }
        );

        assert!(
            document_warnings(&architecture, 10).is_empty(),
            "the warnings belong to the PRD only (ticket 0057)"
        );
    }
}
