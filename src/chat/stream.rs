//! Upstream SSE stream parsing (ticket 0034).

use eventsource_stream::{EventStreamError, Eventsource};
use futures_util::stream::{self, Stream, StreamExt};
use serde_json::Value;

use crate::chat::client::ChatError;

/// The SSE sentinel that ends an OpenAI-compatible stream.
const DONE: &str = "[DONE]";

/// Turns a byte stream into the ordered text deltas of one upstream reply.
///
/// The stream ends cleanly on the `[DONE]` sentinel and ends with a typed
/// error when an event cannot be turned into a delta, so callers never see a
/// stream that fails on its own terms. The full reply is not accumulated here;
/// callers own the accumulation.
pub fn parse<S, B, E>(stream: S) -> impl Stream<Item = Result<String, ChatError>>
where
    S: stream::Stream<Item = Result<B, E>> + Unpin,
    B: AsRef<[u8]>,
{
    stream::unfold(Some(stream.eventsource()), |source| async move {
        let mut source = source?;
        loop {
            let item = source.next().await?;
            let outcome: Option<Result<String, ChatError>> = match item {
                Err(err) => Some(Err(event_error(&err))),
                Ok(event) => {
                    if event.data.trim() == DONE {
                        // The sentinel ends the stream without an error.
                        None
                    } else {
                        match delta(&event.data) {
                            Ok(Some(delta)) => Some(Ok(delta)),
                            // Comments, keep-alives and unknown events yield
                            // nothing and the stream keeps going.
                            Ok(None) => continue,
                            Err(err) => Some(Err(err)),
                        }
                    }
                }
            };

            match outcome {
                None => return None,
                Some(Err(err)) => return Some((Err(err), None)),
                Some(Ok(delta)) => return Some((Ok(delta), Some(source))),
            }
        }
    })
}

/// Maps a failure from the event stream to the typed chat error.
fn event_error<E>(err: &EventStreamError<E>) -> ChatError {
    match err {
        EventStreamError::Utf8(_) | EventStreamError::Parser(_) => ChatError::InvalidResponse {
            message: "the event stream was malformed".to_string(),
        },
        EventStreamError::Transport(_) => ChatError::Network,
    }
}

/// Extracts the text delta from one SSE data payload.
///
/// Returns `Ok(None)` when the event carries no text: the `[DONE]` sentinel, an
/// event without a usable `choices` entry, or a delta that only carries role or
/// finish metadata.
fn delta(data: &str) -> Result<Option<String>, ChatError> {
    let data = data.trim();
    let Ok(value) = serde_json::from_str::<Value>(data) else {
        return Err(ChatError::InvalidResponse {
            message: "the event data was not valid JSON".to_string(),
        });
    };

    if let Some(error) = value.get("error").and_then(Value::as_object) {
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("the upstream reported an error");
        return Err(ChatError::UpstreamError {
            message: message.to_string(),
        });
    }

    let content = value
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("delta"))
        .and_then(Value::as_object)
        .and_then(|delta| delta.get("content"))
        .and_then(Value::as_str);

    Ok(content
        .filter(|content| !content.is_empty())
        .map(str::to_string))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::client::ChatError;
    use futures_util::stream::{self, StreamExt};

    async fn collect(chunks: &[String]) -> Vec<Result<String, ChatError>> {
        let source = stream::iter(
            chunks
                .iter()
                .map(|chunk| Ok::<_, std::io::Error>(chunk.clone()))
                .collect::<Vec<_>>(),
        );
        parse(source).collect().await
    }

    fn data_event(content: &str) -> String {
        format!("data: {content}\n\n")
    }

    fn texts(deltas: Vec<Result<String, ChatError>>) -> Vec<String> {
        deltas
            .into_iter()
            .map(|delta| delta.expect("the parser only yields deltas"))
            .collect()
    }

    #[tokio::test]
    async fn multiple_deltas_arrive_in_order() {
        let chunks = vec![
            data_event(r#"{"choices":[{"delta":{"content":"Hello"}}]}"#),
            data_event(r#"{"choices":[{"delta":{"content":" world"}}]}"#),
            data_event(r#"{"choices":[{"delta":{"content":"!"}}]}"#),
        ];
        let deltas = collect(&chunks).await;

        assert_eq!(
            texts(deltas),
            vec!["Hello".to_string(), " world".to_string(), "!".to_string(),]
        );
    }

    #[tokio::test]
    async fn delta_split_across_chunks_still_parses() {
        let chunks = vec![
            r#"data: {"choices":[{"delta":{"content":"Hel"#.to_string(),
            "lo\"}}]}\n\n".to_string(),
        ];
        let deltas = collect(&chunks).await;

        assert_eq!(texts(deltas), vec!["Hello".to_string()]);
    }

    #[tokio::test]
    async fn utf8_split_across_chunks_is_decoded_correctly() {
        let chunks = vec![
            r#"data: {"choices":[{"delta":{"content":"a"#.to_string(),
            "\u{1F44D}b\"}}]}\n\n".to_string(),
        ];
        let deltas = collect(&chunks).await;

        assert_eq!(texts(deltas), vec!["a\u{1F44D}b".to_string()]);
    }

    #[tokio::test]
    async fn done_ends_the_stream_without_an_error() {
        let chunks = vec![
            data_event(r#"{"choices":[{"delta":{"content":"Hello"}}]}"#),
            "data: [DONE]\n\n".to_string(),
        ];
        let deltas = collect(&chunks).await;

        assert_eq!(texts(deltas), vec!["Hello".to_string()]);
    }

    #[tokio::test]
    async fn done_with_whitespace_ends_the_stream() {
        let chunks = vec![
            data_event(r#"{"choices":[{"delta":{"content":"Hello"}}]}"#),
            "data:   [DONE]  \n\n".to_string(),
        ];
        let deltas = collect(&chunks).await;

        assert_eq!(texts(deltas), vec!["Hello".to_string()]);
    }

    #[tokio::test]
    async fn empty_choices_are_ignored() {
        let chunks = vec![
            data_event(r#"{"choices":[]}"#),
            data_event(r#"{"choices":[{"delta":{"content":"Hello"}}]}"#),
        ];
        let deltas = collect(&chunks).await;

        assert_eq!(texts(deltas), vec!["Hello".to_string()]);
    }

    #[tokio::test]
    async fn malformed_json_emits_one_typed_error_and_ends() {
        let chunks = vec![
            data_event(r#"{"choices":[{"delta":{"content":"Hello"}}]}"#),
            data_event(r#"{"choices":[ not json"#),
        ];
        let deltas = collect(&chunks).await;

        assert_eq!(deltas.len(), 2);
        assert!(matches!(deltas[1], Err(ChatError::InvalidResponse { .. })));
    }

    #[tokio::test]
    async fn error_object_yields_an_upstream_error() {
        let chunks = vec![data_event(
            r#"{"error":{"message":"boom","type":"server_error"}}"#,
        )];
        let deltas = collect(&chunks).await;

        assert!(matches!(deltas[0], Err(ChatError::UpstreamError { .. })));
    }

    #[tokio::test]
    async fn final_chunk_with_finish_reason_and_empty_content_is_ignored() {
        let chunks = vec![data_event(
            r#"{"choices":[{"delta":{"content":""},"finish_reason":"stop"}]}"#,
        )];
        let deltas = collect(&chunks).await;

        assert!(deltas.is_empty());
    }

    #[tokio::test]
    async fn empty_stream_yields_no_items_without_panicking() {
        let empty: Vec<String> = Vec::new();
        let deltas = collect(&empty).await;

        assert!(deltas.is_empty());
    }

    #[tokio::test]
    async fn crlf_frames_data_without_space_and_comments_parse() {
        let chunks = vec![
            ": keep-alive\n\n".to_string(),
            "event: message\r\ndata:{\"choices\":[{\"delta\":{\"content\":\"Hi\"}}]}\r\n\r\n"
                .to_string(),
            "data: {\"choices\": [\ndata: {\"delta\": {\"content\": \"Hello\"}}\ndata: ]}\n\n"
                .to_string(),
        ];
        let deltas = collect(&chunks).await;

        assert_eq!(texts(deltas), vec!["Hi".to_string(), "Hello".to_string()]);
    }
}
