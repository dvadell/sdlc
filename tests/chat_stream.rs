//! Integration tests for the upstream stream parser (ticket 0034).

use futures_util::stream::StreamExt;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use sdlc::chat::client::{ChatClient, ChatMessage};
use sdlc::chat::stream::parse;
use sdlc::config::{AiConfig, ApiKey};

fn client(base_url: &str, api_key: Option<String>) -> ChatClient {
    let ai = AiConfig::new(
        base_url.to_string(),
        "gpt-4o".to_string(),
        api_key.map(ApiKey::new),
    );
    ChatClient::new(&ai).expect("the chat client should build")
}

fn message(content: &str) -> ChatMessage {
    ChatMessage {
        role: "user".to_string(),
        content: content.to_string(),
    }
}

#[tokio::test]
async fn streams_three_deltas_and_done() {
    let mock_server = MockServer::start().await;

    let body = "data: {\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}\n\n\
                data: {\"choices\":[{\"delta\":{\"content\":\" world\"}}]}\n\n\
                data: {\"choices\":[{\"delta\":{\"content\":\"!\"}}]}\n\n\
                data: [DONE]\n\n";
    let mock = Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(body.as_bytes().to_vec(), "text/event-stream"),
        );
    mock.mount(&mock_server).await;

    let client = client(&mock_server.uri(), None);
    let response = client
        .send(&[message("hello")])
        .await
        .expect("the upstream should accept the request");
    let deltas = parse(response.bytes_stream()).collect::<Vec<_>>().await;

    mock_server.verify().await;

    let texts = deltas
        .into_iter()
        .map(|delta| delta.expect("the parser only yields deltas"))
        .collect::<Vec<_>>();
    assert_eq!(texts, vec!["Hello", " world", "!"]);
}
