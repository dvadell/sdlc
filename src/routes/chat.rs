//! Chat SSE route (ticket 0036).
//!
//! `POST /api/chat` validates the conversation, loads the referenced documents
//! and relays the upstream reply as Server-Sent Events that always terminate.

use std::collections::VecDeque;
use std::pin::Pin;
use std::sync::Arc;

use axum::{
    Json,
    body::Body,
    extract::{State, rejection::JsonRejection},
    http::{HeaderName, HeaderValue, StatusCode, header},
    response::Response,
};
use futures_util::stream::{self, Stream, StreamExt};
use serde::Deserialize;

use crate::chat::{
    ChatError, ExistingTicket, Message, Parsed, TicketContext, blocks, build, parse,
};
use crate::error::ApiError;
use crate::proposal::{Action, Proposal, ProposalService};
use crate::state::AppState;
use crate::store::{DocKind, DocPath, Document, StoreError};

/// Maximum accepted chat request body: 64 KiB, enough for a bounded
/// conversation and small documents, small enough to fail oversized payloads
/// early.
pub(crate) const MAX_CHAT_BODY_BYTES: usize = 64 * 1024;

/// Maximum characters of one conversation turn's content.
const MAX_MESSAGE_CHARS: usize = 8192;

/// Handles `POST /api/chat`.
///
/// The request is validated before anything is streamed; an invalid request
/// answers with the JSON error envelope, never with an SSE stream. A missing AI
/// configuration answers with `503 ai_not_configured` before a stream opens.
/// The upstream reply is relayed frame by frame and the stream always ends:
/// the final `done` frame on a clean end, one `error` frame on a failure. A
/// clean reply is parsed for proposal blocks afterwards: the skipped blocks are
/// answered with `warning` frames and a built proposal with one `proposal` frame
/// before the `done` frame.
pub(crate) async fn chat(
    State(state): State<AppState>,
    body: Result<Json<ChatRequest>, JsonRejection>,
) -> Result<Response, ApiError> {
    let request = super::docs::json_body(body)?;

    if state.config.ai().is_none() {
        return Err(ApiError::ai_not_configured());
    }

    let turns = validate_request(&request)?;
    let open = load_document(&state, &turns.open_doc).await?;
    let attachments = load_attachments(&state, &turns.attachments).await;

    let ticket_context = existing_tickets(&state, turns.ticket_context.as_ref()).await;

    let messages = build(&open, &attachments, turns.history, ticket_context.as_ref());
    let source: Pin<Box<dyn Stream<Item = Result<String, ChatError>> + Send>> =
        match state.chat.send(&messages).await {
            Ok(response) => Box::pin(parse(response.bytes_stream())),
            Err(error) => Box::pin(stream::once(async move { Err::<String, ChatError>(error) })),
        };

    Ok(sse_response(frames(
        source,
        state.proposals.clone(),
        ticket_context,
    )))
}

/// Attaches the tickets that already exist for the feature in the selected epic
/// to the ticket-generation context of a turn (ticket 0051).
///
/// The feature and the epic of the context select the regeneration scope: the
/// tickets of that feature under that epic are loaded from the store, in ticket
/// number order, and carried as the existing tickets of the context, so the
/// prompt of the regeneration and the proposal it builds see them. A turn
/// without a context, or with an incomplete one, keeps its plain context, and the
/// tickets of other features or epics are never part of it.
async fn existing_tickets(
    state: &AppState,
    context: Option<&TicketContext>,
) -> Option<TicketContext> {
    let context = context.as_ref()?;
    let context = TicketContext {
        feature: context.feature.clone(),
        epic: context.epic.clone(),
        existing: context.existing.clone(),
    };
    let Some(epic) = context.epic.as_deref() else {
        return Some(context);
    };
    let Some(feature) = context.feature.as_deref() else {
        return Some(context);
    };

    let tree = state.store.tree().await;
    let mut existing = Vec::new();
    for entry in &tree.docs {
        if entry.kind != DocKind::Ticket
            || entry.epic.as_deref() != Some(epic)
            || entry.feature.as_deref() != Some(feature)
        {
            continue;
        }
        if let Ok(document) = state.store.read(&entry.path).await {
            existing.push(ExistingTicket {
                path: entry.path.as_path().to_string_lossy().into_owned(),
                content: document.content,
            });
        }
    }

    Some(TicketContext {
        feature: context.feature.clone(),
        epic: context.epic.clone(),
        existing,
    })
}

/// The body of a chat request.
#[derive(Debug, Deserialize)]
pub(crate) struct ChatRequest {
    /// The conversation turns, oldest first; the first is the current turn.
    pub(crate) messages: Vec<ChatTurn>,
    /// The focused document path.
    pub(crate) open_doc: String,
    /// The optional attached document paths.
    #[serde(default)]
    pub(crate) attachments: Vec<String>,
    /// The optional ticket-generation context: the feature slug and the epic
    /// directory the next tickets are drafted for, plus the tickets that already
    /// exist for a regeneration (tickets 0049 and 0051).
    #[serde(default)]
    pub(crate) ticket_context: Option<TicketContext>,
}

/// One conversation turn of a chat request.
#[derive(Debug, Deserialize)]
pub(crate) struct ChatTurn {
    /// The OpenAI-style role; only `user` and `assistant` are accepted.
    pub(crate) role: String,
    /// The turn content.
    pub(crate) content: String,
}

/// The validated chat request before the store is touched.
struct ValidatedChat {
    /// The conversation turns, without the context turn added by the builder.
    history: Vec<Message>,
    /// The focused document path.
    open_doc: DocPath,
    /// The attached paths that parsed as documents, missing ones skipped later.
    attachments: Vec<DocPath>,
    /// The ticket-generation context of ticket 0049, when the turn drafts
    /// tickets.
    ticket_context: Option<TicketContext>,
}

/// Validates the chat request, rejecting every shape the route refuses before
/// a stream opens.
fn validate_request(request: &ChatRequest) -> Result<ValidatedChat, ApiError> {
    if request.messages.is_empty() {
        return Err(ApiError::bad_request());
    }

    let mut history = Vec::with_capacity(request.messages.len());
    for turn in &request.messages {
        if turn.role != "user" && turn.role != "assistant" {
            // The context lives in the system prompt built by the prompt
            // builder, so a client-supplied `system` turn is refused here
            // instead of reaching the upstream.
            return Err(ApiError::bad_request());
        }
        if turn.content.chars().count() > MAX_MESSAGE_CHARS {
            return Err(ApiError::bad_request());
        }
        history.push(Message {
            role: turn.role.clone(),
            content: turn.content.clone(),
        });
    }

    let open_doc = DocPath::parse(&request.open_doc)?;
    let mut attachments = Vec::with_capacity(request.attachments.len());
    for attachment in &request.attachments {
        attachments.push(DocPath::parse(attachment)?);
    }

    Ok(ValidatedChat {
        history,
        open_doc,
        attachments,
        ticket_context: validate_ticket_context(&request.ticket_context)?,
    })
}

/// Validates the optional ticket-generation context, or `None` when the turn
/// does not draft tickets.
///
/// The feature is a slug without the `.md` suffix and the epic the exact epic
/// directory name such as `0001-foundation`, so both are checked against the
/// document layout before they reach the prompt builder and the proposal creation
/// (ticket 0049). An invalid context is a bad request rather than an ordinary turn,
/// so it never reaches the model or the proposal layer.
fn validate_ticket_context(
    context: &Option<TicketContext>,
) -> Result<Option<TicketContext>, ApiError> {
    let Some(context) = context.as_ref() else {
        return Ok(None);
    };
    let feature = context
        .feature
        .as_deref()
        .map(str::trim)
        .filter(|feature| !feature.is_empty())
        .map(str::to_string);
    let epic = context
        .epic
        .as_deref()
        .map(str::trim)
        .filter(|epic| !epic.is_empty())
        .map(str::to_string);

    if let Some(feature) = &feature
        && DocPath::parse(&format!("sdlc/features/{feature}.md")).is_err()
    {
        return Err(ApiError::bad_request());
    }
    if let Some(epic) = &epic
        && DocPath::parse(&format!("sdlc/epics/{epic}/description.md")).is_err()
    {
        return Err(ApiError::bad_request());
    }

    if feature.is_none() && epic.is_none() {
        // A provided context with no usable fields is a no-op; downgrade it to a
        // plain turn rather than a context that skips epic enforcement.
        return Ok(None);
    }

    Ok(Some(TicketContext {
        feature,
        epic,
        existing: Vec::new(),
    }))
}

/// Loads the open document of the conversation.
///
/// The open document is required, so a missing one is a bad request rather than
/// a missing resource; every other store failure keeps its own mapping.
async fn load_document(state: &AppState, path: &DocPath) -> Result<Document, ApiError> {
    match state.store.read(path).await {
        Ok(document) => Ok(document),
        Err(StoreError::NotFound(_)) => Err(ApiError::bad_request()),
        Err(error) => Err(ApiError::from(error)),
    }
}

/// Loads the referenced attachments; a missing one is skipped by the prompt.
async fn load_attachments(state: &AppState, paths: &[DocPath]) -> Vec<Document> {
    let mut documents = Vec::with_capacity(paths.len());
    for path in paths {
        if let Ok(document) = state.store.read(path).await {
            documents.push(document);
        }
    }
    documents
}

/// Wraps a frame stream into the SSE response with the required headers.
///
/// The response is uncacheable and never buffered by proxies, so a browser
/// reader and an `EventSource` both see every frame as it is produced.
fn sse_response(
    response: impl Stream<Item = Result<String, ChatError>> + Send + 'static,
) -> Response {
    let mut response = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .body(Body::from_stream(response))
        .expect("the SSE response body should build");

    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    response.headers_mut().insert(
        HeaderName::from_static("x-accel-buffering"),
        HeaderValue::from_static("no"),
    );

    response
}

/// The state of the SSE relay: it is streaming upstream deltas, emitting the
/// final frames of a clean end or finished.
enum RelayState {
    /// The upstream deltas are being turned into frames.
    Streaming {
        source: Pin<Box<dyn Stream<Item = Result<String, ChatError>> + Send>>,
        /// The full reply so far, accumulated for the later proposal ticket.
        accumulated: String,
    },
    /// The reply ended cleanly and its terminal frames were computed; they are
    /// queued in order and emitted one by one until the queue drains.
    Finalizing {
        frames: VecDeque<Result<String, ChatError>>,
    },
    /// The final frame was emitted, so the stream ends.
    Finished,
}

/// Turns the ordered upstream deltas into the SSE frames of one reply.
///
/// The stream ends after the `done` frame of a clean end and after the `error`
/// frame of a failure, so a client reader never waits for bytes that do not
/// come. The source is boxed because both the upstream stream and the failed
/// send case feed the same relay; the proposal service turns the accumulated
/// reply into the terminal frames once the upstream ended without error.
fn frames(
    source: Pin<Box<dyn Stream<Item = Result<String, ChatError>> + Send>>,
    proposals: Arc<ProposalService>,
    ticket_context: Option<TicketContext>,
) -> impl Stream<Item = Result<String, ChatError>> + Send + 'static {
    stream::unfold(
        RelayState::Streaming {
            source,
            accumulated: String::new(),
        },
        move |state| {
            let value = proposals.clone();
            // The ticket context is only needed once the upstream reply ended
            // cleanly, so the clone lives with this closure step instead of
            // moving the captured context into the async block per upstream delta.
            let ticket_context = ticket_context.clone();
            async move {
                let proposals = value;
                let (frame, next_state) = match state {
                    RelayState::Finished => return None,
                    RelayState::Finalizing { mut frames } => {
                        let Some(frame) = frames.pop_front() else {
                            // The terminal frame was the last queued one, so the
                            // stream ends here.
                            return None;
                        };
                        (frame, RelayState::Finalizing { frames })
                    }
                    RelayState::Streaming {
                        mut source,
                        mut accumulated,
                    } => {
                        let item = source.next().await;
                        let Some(item) = item else {
                            // The clean end of the reply closes the stream with the
                            // final frames: the warnings, the proposal and the `done`
                            // frame, or the `error` frame of a refused proposal. A
                            // reply that ended in an error never reaches this branch,
                            // so a partial reply is never parsed.
                            let parsed = blocks::parse(&accumulated);
                            let mut frames: VecDeque<Result<String, ChatError>> =
                                finalize_reply(&parsed, &proposals, ticket_context.as_ref())
                                    .await
                                    .into_iter()
                                    .collect();
                            let Some(frame) = frames.pop_front() else {
                                // `finalize_reply` always queues the terminal frame of a
                                // reply before returning, so the queue cannot drain
                                // before the proposal decision is made.
                                return None;
                            };
                            return Some((frame, RelayState::Finalizing { frames }));
                        };

                        let (frame, next_state) = match item {
                            Err(error) => (frame_error(&error), RelayState::Finished),
                            Ok(delta) => {
                                accumulated.push_str(&delta);
                                (
                                    frame_delta(&delta),
                                    RelayState::Streaming {
                                        source,
                                        accumulated,
                                    },
                                )
                            }
                        };

                        (Ok(frame), next_state)
                    }
                };

                Some((frame, next_state))
            }
        },
    )
}

/// Turns the parsed reply blocks into the terminal frames of a clean end.
///
/// The frames are the warnings of the skipped blocks, then the `proposal` frame
/// of a built proposal, then the `done` frame; a refused proposal is answered
/// with the `error` frame alone, so the stream always ends with one terminal
/// event and never with `done` after an error.
async fn finalize_reply(
    parsed: &Parsed,
    proposals: &ProposalService,
    ticket_context: Option<&TicketContext>,
) -> Vec<Result<String, ChatError>> {
    let mut frames = Vec::new();

    for warning in &parsed.warnings {
        frames.push(Ok(frame_warning(warning)));
    }

    if parsed.blocks.is_empty() {
        // A reply without blocks is ordinary chat text; its deltas were already
        // streamed and the stream closes with `done`.
        frames.push(Ok(frame_done()));
        return frames;
    }

    match proposals.create(&parsed.blocks, ticket_context).await {
        Ok(proposal) => {
            // The proposal warnings carry the ticket numbering of ticket 0050,
            // on top of the warnings of the skipped blocks of the reply.
            let mut warnings = parsed.warnings.clone();
            warnings.extend(proposal.warnings.iter().cloned());
            frames.push(Ok(frame_proposal(&proposal, &warnings)));
            frames.push(Ok(frame_done()));
        }
        Err(error) => frames.push(Ok(frame_error(&ChatError::from(error)))),
    }

    frames
}

/// The SSE frame that relays one text delta of the reply.
fn frame_delta(text: &str) -> String {
    format!(
        "event: delta\ndata: {}\n\n",
        serde_json::json!({ "text": text })
    )
}

/// The SSE frame that reports a failure and ends the stream.
fn frame_error(error: &ChatError) -> String {
    format!(
        "event: error\ndata: {}\n\n",
        serde_json::json!({
            "code": error_code(error),
            "message": error.to_string(),
        })
    )
}

/// The SSE frame that relays one warning of a skipped proposal block.
fn frame_warning(message: &str) -> String {
    format!(
        "event: warning\ndata: {}\n\n",
        serde_json::json!({ "message": message })
    )
}

/// The SSE frame that relays the proposal built from a clean reply, with the
/// warnings of the skipped blocks beside it.
fn frame_proposal(proposal: &Proposal, warnings: &[String]) -> String {
    let changes = proposal
        .changes
        .iter()
        .map(|change| {
            serde_json::json!({
                "path": change.path.as_path().to_string_lossy(),
                "action": action_name(change.action),
                "diff": change.diff,
            })
        })
        .collect::<Vec<serde_json::Value>>();

    format!(
        "event: proposal\ndata: {}\n\n",
        serde_json::json!({
            "id": proposal.id,
            "changes": changes,
            "warnings": warnings,
        })
    )
}

/// The final SSE frame that closes a clean reply.
fn frame_done() -> String {
    "event: done\ndata: {}\n\n".to_string()
}

/// The proposal action name of one change, for the `proposal` event.
fn action_name(action: Action) -> &'static str {
    match action {
        Action::Create => "create",
        Action::Modify => "modify",
        Action::Delete => "delete",
    }
}

/// The stable machine-readable code of a chat failure, for the `error` event.
fn error_code(error: &ChatError) -> &'static str {
    match error {
        ChatError::NotConfigured => "not_configured",
        ChatError::Unauthorized => "unauthorized",
        ChatError::Upstream { .. } => "upstream_error",
        ChatError::Network => "network",
        ChatError::Truncated => "truncated",
        ChatError::InvalidResponse { .. } => "invalid_response",
        ChatError::UpstreamError { .. } => "upstream_stream_error",
        ChatError::InvalidBaseUrl { .. } => "invalid_base_url",
        ChatError::Proposal { code, .. } => code,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proposal::{Change, ProposalError};

    #[test]
    fn delta_frame_carries_the_text_in_a_json_data_event() {
        assert_eq!(
            frame_delta("Hello"),
            "event: delta\ndata: {\"text\":\"Hello\"}\n\n"
        );
    }

    #[test]
    fn done_frame_is_an_empty_object() {
        assert_eq!(frame_done(), "event: done\ndata: {}\n\n");
    }

    #[test]
    fn error_frame_carries_a_stable_code_and_a_short_message() {
        let error = ChatError::Upstream {
            status: 500,
            snippet: "boom".to_string(),
        };

        assert_eq!(
            frame_error(&error),
            "event: error\ndata: {\"code\":\"upstream_error\",\"message\":\"chat upstream returned status 500\"}\n\n"
        );
    }

    #[test]
    fn error_frame_of_an_upstream_stream_error_uses_the_stream_error_code() {
        let error = ChatError::UpstreamError {
            message: "boom".to_string(),
        };

        assert_eq!(
            frame_error(&error),
            "event: error\ndata: {\"code\":\"upstream_stream_error\",\"message\":\"chat upstream returned an error: boom\"}\n\n"
        );
    }

    #[test]
    fn warning_frame_carries_one_message() {
        assert_eq!(
            frame_warning("the block was skipped"),
            "event: warning\ndata: {\"message\":\"the block was skipped\"}\n\n"
        );
    }

    #[test]
    fn error_frame_of_a_refused_proposal_uses_the_refusal_code() {
        let error = ChatError::from(ProposalError::InvalidPath("../AGENTS.md".to_string()));

        assert_eq!(
            error_code(&error),
            "invalid_path",
            "the refusal should answer with its own code: {error}"
        );
        assert_eq!(
            frame_error(&error),
            "event: error\ndata: {\"code\":\"invalid_path\",\"message\":\"the proposed path is not an allowed document: ../AGENTS.md\"}\n\n"
        );
    }

    #[test]
    fn proposal_frame_carries_the_id_the_changes_and_the_warnings() {
        let change = Change {
            path: DocPath::parse("sdlc/features/export.md").expect("the path should be valid"),
            action: Action::Create,
            new_content: Some("# Export\n".to_string()),
            base_hash: None,
            diff: String::new(),
        };
        let proposal = Proposal {
            id: "0d1b2f3a-4b5c-6d7e-8f90-1a2b3c4d5e6f".to_string(),
            changes: vec![change],
            warnings: Vec::new(),
        };

        let frame = frame_proposal(&proposal, &["the block was skipped".to_string()]);
        let data = frame
            .strip_prefix("event: proposal\ndata: ")
            .and_then(|data| data.strip_suffix("\n\n"))
            .expect("the proposal frame should carry one data line");
        let value: serde_json::Value =
            serde_json::from_str(data).expect("the proposal data should be JSON");

        assert_eq!(value["id"], "0d1b2f3a-4b5c-6d7e-8f90-1a2b3c4d5e6f");
        let changes = value["changes"].as_array().expect("the changes");
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0]["path"], "sdlc/features/export.md");
        assert_eq!(changes[0]["action"], "create");
        assert_eq!(changes[0]["diff"], "");
        assert_eq!(value["warnings"][0], "the block was skipped");
    }
}
