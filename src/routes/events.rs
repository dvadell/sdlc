//! The file-change SSE route (ticket 0056).
//!
//! `GET /api/events` answers with the Server-Sent Events stream of the file
//! change hub: one `file-changed` frame per debounced document change, plus a
//! comment heartbeat every 15 seconds so a local proxy or an idle connection does
//! not close an otherwise silent stream. The stream ends when the client
//! disconnects, because the response body is dropped with the connection.

use std::pin::Pin;
use std::time::Duration;

use axum::{
    body::Body,
    extract::State,
    http::{HeaderName, HeaderValue, StatusCode, header},
    response::Response,
};
use futures_util::stream::{self, Stream};
use serde_json::json;
use tokio::sync::broadcast;
use tokio::time::timeout_at;

use crate::state::AppState;
use crate::watch::FileChanged;

/// The heartbeat interval of the event stream: a comment frame every 15 seconds
/// keeps a local proxy or a quiet connection from reaping the stream (ticket 0056).
const HEARTBEAT: Duration = Duration::from_secs(15);

/// The comment frame of the heartbeat: SSE comments start with `:` and carry no
/// data, so a proxy or a browser reader ignores them as keep-alive traffic
/// (ticket 0056).
const HEARTBEAT_FRAME: &str = ": heartbeat\n\n";

/// The event stream of one subscriber of the file change hub.
///
/// The stream only ever yields frames or ends, so its error type is
/// [`std::convert::Infallible`] and the never-failing contract is explicit (ticket
/// 0056).
type EventStream = Pin<Box<dyn Stream<Item = Result<String, std::convert::Infallible>> + Send>>;

/// Handles `GET /api/events`.
///
/// The response is uncacheable and never buffered, so a browser reader and an
/// `EventSource` both see every frame as it is produced; the stream ends when the
/// client disconnects.
pub async fn get_events(State(state): State<AppState>) -> Response {
    let stream: EventStream = Box::pin(file_events(&state));

    let mut response = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .body(Body::from_stream(stream))
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

/// The `file-changed` frame of one debounced document change (ticket 0054).
fn file_changed_frame(path: &str) -> String {
    format!("event: file-changed\ndata: {}\n\n", json!({ "path": path }))
}

/// The event stream of one subscriber of the file change hub.
///
/// The subscriber drains the broadcast receiver until it closes, emitting one
/// frame per published change and one comment heartbeat per interval; a subscriber
/// that falls behind resyncs from the current tail instead of failing, exactly like
/// the one-shot receive of the hub (ticket 0054). The stream ends when the hub
/// closes it, and is dropped with the client connection when the reader goes away.
fn file_events(state: &AppState) -> EventStream {
    let initial = EventStreamState {
        receiver: state.events.subscribe(),
        deadline: tokio::time::Instant::now() + HEARTBEAT,
    };
    Box::pin(stream::unfold(initial, |mut state| async move {
        loop {
            let deadline = state.deadline;
            let received = timeout_at(deadline, state.receiver.recv()).await;
            match received {
                Ok(Ok(changed)) => {
                    state.deadline = tokio::time::Instant::now() + HEARTBEAT;
                    return Some((Ok(file_changed_frame(&changed.path)), state));
                }
                Ok(Err(broadcast::error::RecvError::Lagged(missed))) => {
                    tracing::warn!(missed, "the file event subscriber lagged behind; resyncing");
                    state.receiver = state.receiver.resubscribe();
                    state.deadline = tokio::time::Instant::now() + HEARTBEAT;
                }
                Ok(Err(broadcast::error::RecvError::Closed)) => {
                    // The hub closed the subscriber, so the stream ends.
                    return None;
                }
                Err(_elapsed) => {
                    state.deadline = tokio::time::Instant::now() + HEARTBEAT;
                    return Some((Ok(HEARTBEAT_FRAME.to_string()), state));
                }
            }
        }
    }))
}

/// The state of one SSE subscriber: the receiver to drain and the deadline of the
/// next heartbeat comment.
struct EventStreamState {
    receiver: broadcast::Receiver<FileChanged>,
    deadline: tokio::time::Instant,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_changed_frame_carries_the_path_in_json_data() {
        assert_eq!(
            file_changed_frame("sdlc/PRD.md"),
            "event: file-changed\ndata: {\"path\":\"sdlc/PRD.md\"}\n\n"
        );
    }

    #[test]
    fn heartbeat_frame_is_a_comment_without_data() {
        assert_eq!(HEARTBEAT_FRAME, ": heartbeat\n\n");
    }
}
