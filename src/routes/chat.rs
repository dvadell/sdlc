//! Chat SSE route (ticket 0036).
//!
//! `POST /api/chat` validates the conversation, loads the referenced documents
//! and relays the upstream reply as Server-Sent Events that always terminate.

use std::pin::Pin;

use axum::{
    Json,
    body::Body,
    extract::{State, rejection::JsonRejection},
    http::{HeaderName, HeaderValue, StatusCode, header},
    response::Response,
};
use futures_util::stream::{self, Stream, StreamExt};
use serde::Deserialize;

use crate::chat::{ChatError, Message, build, parse};
use crate::error::ApiError;
use crate::state::AppState;
use crate::store::{DocPath, Document, StoreError};

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
/// the final `done` frame on a clean end, one `error` frame on a failure.
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

    let messages = build(&open, &attachments, turns.history);
    let source: Pin<Box<dyn Stream<Item = Result<String, ChatError>> + Send>> =
        match state.chat.send(&messages).await {
            Ok(response) => Box::pin(parse(response.bytes_stream())),
            Err(error) => Box::pin(stream::once(async move { Err::<String, ChatError>(error) })),
        };

    Ok(sse_response(frames(source)))
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
    })
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

/// The state of the SSE relay: it is streaming upstream deltas or finished.
enum RelayState {
    /// The upstream deltas are being turned into frames.
    Streaming {
        source: Pin<Box<dyn Stream<Item = Result<String, ChatError>> + Send>>,
        /// The full reply so far, accumulated for the later proposal ticket.
        accumulated: String,
    },
    /// The reply ended; the frame was already emitted, so the stream ends.
    Finished,
}

/// Turns the ordered upstream deltas into the SSE frames of one reply.
///
/// The stream ends after the `done` frame of a clean end and after the `error`
/// frame of a failure, so a client reader never waits for bytes that do not
/// come. The source is boxed because both the upstream stream and the failed
/// send case feed the same relay.
fn frames(
    source: Pin<Box<dyn Stream<Item = Result<String, ChatError>> + Send>>,
) -> impl Stream<Item = Result<String, ChatError>> + Send + 'static {
    stream::unfold(
        RelayState::Streaming {
            source,
            accumulated: String::new(),
        },
        |state| async move {
            let RelayState::Streaming {
                mut source,
                mut accumulated,
            } = state
            else {
                // The final frame was emitted, so the stream ends here.
                return None;
            };

            let item = source.next().await;
            let Some(item) = item else {
                // The clean end of the reply closes the stream with `done`.
                tracing::debug!(
                    chars = accumulated.chars().count(),
                    "the chat reply finished"
                );
                return Some((Ok(frame_done()), RelayState::Finished));
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

            Some((Ok(frame), next_state))
        },
    )
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

/// The final SSE frame that closes a clean reply.
fn frame_done() -> String {
    "event: done\ndata: {}\n\n".to_string()
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
