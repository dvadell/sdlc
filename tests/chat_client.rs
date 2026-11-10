//! Integration tests for the upstream chat client (ticket 0033).

use wiremock::matchers::{body_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use sdlc::chat::client::{ChatClient, ChatError, ChatMessage};
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
async fn send_posts_the_completions_url_with_model_and_stream() {
    let mock_server = MockServer::start().await;

    let mock = Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(body_json(serde_json::json!({
            "model": "gpt-4o",
            "messages": [{ "role": "user", "content": "hello" }],
            "stream": true,
        })))
        .respond_with(ResponseTemplate::new(200).set_body_string("ok"));
    mock.mount(&mock_server).await;

    let client = client(&mock_server.uri(), Some("sk-test".to_string()));
    let response = client
        .send(&[message("hello")])
        .await
        .expect("the upstream should accept the request");

    mock_server.verify().await;
    assert_eq!(response.status(), 200);
}

#[tokio::test]
async fn send_sends_the_bearer_token_when_configured() {
    let mock_server = MockServer::start().await;

    let mock = Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(header("Authorization", "Bearer sk-test"))
        .respond_with(ResponseTemplate::new(200).set_body_string("ok"));
    mock.mount(&mock_server).await;

    let client = client(&mock_server.uri(), Some("sk-test".to_string()));
    let response = client
        .send(&[message("hello")])
        .await
        .expect("the upstream should accept the request");

    mock_server.verify().await;
    assert_eq!(response.status(), 200);
}

#[tokio::test]
async fn send_sends_no_bearer_token_without_a_key() {
    let mock_server = MockServer::start().await;

    let mock = Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_string("ok"));
    mock.mount(&mock_server).await;

    let client = client(&mock_server.uri(), None);
    let response = client
        .send(&[message("hello")])
        .await
        .expect("the upstream should accept the request");

    mock_server.verify().await;
    assert_eq!(response.status(), 200);

    let received = mock_server.received_requests().await.expect("record");
    let request = received
        .into_iter()
        .find(|request| request.url.path() == "/chat/completions")
        .expect("the chat completion request should be recorded");

    assert!(request.headers.get("authorization").is_none());
}

#[tokio::test]
async fn send_maps_401_to_unauthorized() {
    let mock_server = MockServer::start().await;

    let mock = Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(401).set_body_json(serde_json::json!({
            "error": { "message": "bad key" }
        })));
    mock.mount(&mock_server).await;

    let client = client(&mock_server.uri(), Some("sk-test".to_string()));
    let err = client
        .send(&[message("hello")])
        .await
        .expect_err("a 401 should be reported");

    assert!(matches!(err, ChatError::Unauthorized));
}

#[tokio::test]
async fn send_maps_500_to_upstream_with_a_truncated_snippet() {
    let mock_server = MockServer::start().await;

    let mock = Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(500).set_body_json(serde_json::json!({
            "error": { "message": "boom" }
        })));
    mock.mount(&mock_server).await;

    let client = client(&mock_server.uri(), None);
    let err = client
        .send(&[message("hello")])
        .await
        .expect_err("a 500 should be reported");

    match err {
        ChatError::Upstream { status, snippet } => {
            assert_eq!(status, 500);
            assert!(!snippet.is_empty());
        }
        other => panic!("expected the upstream error, got {other:?}"),
    }
}

#[tokio::test]
async fn send_maps_invalid_json_response_to_invalid_response() {
    let mock_server = MockServer::start().await;

    let mock = Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(b"not json".to_vec(), "application/json"),
        );
    mock.mount(&mock_server).await;

    let client = client(&mock_server.uri(), None);
    let err = client
        .send(&[message("hello")])
        .await
        .expect_err("an unparseable JSON body should be reported");

    assert!(matches!(err, ChatError::InvalidResponse));
}

#[tokio::test]
async fn send_maps_connection_refused_to_network() {
    // An invalid port is refused by the kernel, so no listener can race a
    // just-freed ephemeral port here.
    let client = client("http://127.0.0.1:1", None);
    let err = client
        .send(&[message("hello")])
        .await
        .expect_err("a refused connection should be reported");

    assert!(matches!(err, ChatError::Network));
}
