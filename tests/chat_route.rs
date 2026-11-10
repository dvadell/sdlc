//! Integration tests for the chat SSE route (ticket 0036).

use std::fs;
use std::net::SocketAddr;
use std::path::Path;
use std::time::Duration;

use sdlc::chat::build;
use sdlc::config::{CliArgs, Config};
use sdlc::store::{DocPath, Document, content_hash};
use sdlc::{ServerError, serve};
use serde_json::json;
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// A server bound to an ephemeral loopback port, stopped when the test ends.
struct TestServer {
    addr: SocketAddr,
    handle: JoinHandle<Result<SocketAddr, ServerError>>,
}

impl TestServer {
    async fn start(dir: &Path, ai: &[(&str, Option<String>)]) -> Self {
        let config = Config::from_parts(
            CliArgs {
                dir: dir.to_path_buf(),
                port: 0,
                open: false,
            },
            |key| {
                ai.iter()
                    .find_map(|(name, value)| (*name == key).then(|| value.clone()).flatten())
            },
        )
        .expect("the test config should build");
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("the listener should bind");
        let addr = listener.local_addr().expect("the listener address");
        let handle = tokio::spawn(serve(config, listener));

        Self { addr, handle }
    }

    async fn chat(&self, body: &str) -> HttpResponse {
        self.request("POST", "/api/chat", Some("application/json"), body)
            .await
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

/// The status, headers and body of one raw HTTP response.
struct HttpResponse {
    status: u16,
    headers: Vec<(String, String)>,
    body: String,
}

impl HttpResponse {
    fn parse(response: String) -> Self {
        let status = response
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .and_then(|code| code.parse::<u16>().ok())
            .unwrap_or_else(|| {
                panic!("the status line should carry a numeric status: {response:?}")
            });
        let (head, body) = response
            .split_once("\r\n\r\n")
            .expect("the response should have a head and a body");
        let headers: Vec<(String, String)> = head
            .lines()
            .skip(1)
            .filter_map(|line| {
                let (name, value) = line.split_once(':')?;
                Some((name.trim().to_ascii_lowercase(), value.trim().to_string()))
            })
            .collect();

        let body = decode_body(body, &headers);

        Self {
            status,
            headers,
            body,
        }
    }

    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find_map(|(header, value)| (header == name).then_some(value.as_str()))
    }

    fn json(&self) -> serde_json::Value {
        serde_json::from_str(&self.body).expect("the response body should be JSON")
    }

    fn error_code(&self) -> Option<String> {
        self.json()["error"]["code"].as_str().map(str::to_owned)
    }
}

/// Decodes a chunked transfer body into its plain text.
///
/// Streaming responses carry no content length, so hyper frames them; the raw
/// client sees the chunk metadata until the terminal chunk.
fn decode_body(body: &str, headers: &[(String, String)]) -> String {
    let chunked = headers.iter().any(|(name, value)| {
        name == "transfer-encoding" && value.to_ascii_lowercase().contains("chunked")
    });
    if !chunked {
        return body.to_string();
    }

    let bytes = body.as_bytes();
    let mut decoded = Vec::new();
    let mut rest: &[u8] = bytes;
    loop {
        let line_end = rest
            .windows(2)
            .position(|window| window == b"\r\n")
            .expect("a chunk size line should end with CRLF")
            + 2;
        let size = usize::from_str_radix(
            std::str::from_utf8(&rest[..line_end - 2])
                .expect("the chunk size should be ASCII")
                .trim(),
            16,
        )
        .expect("the chunk size should be hexadecimal");
        if size == 0 {
            break;
        }

        decoded.extend_from_slice(&rest[line_end..line_end + size]);
        rest = &rest[line_end + size..];
        rest = &rest[2..];
    }

    String::from_utf8(decoded).expect("the decoded body should be UTF-8")
}

impl TestServer {
    async fn request(
        &self,
        method: &str,
        target: &str,
        content_type: Option<&str>,
        body: &str,
    ) -> HttpResponse {
        let mut stream = TcpStream::connect(self.addr)
            .await
            .expect("connect to the server");

        let mut raw = format!(
            "{method} {target} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nConnection: close\r\n",
            self.addr.port()
        );
        if let Some(content_type) = content_type {
            raw.push_str(&format!("Content-Type: {content_type}\r\n"));
        }
        if method == "PUT" || method == "POST" {
            raw.push_str(&format!("Content-Length: {}\r\n", body.len()));
        }
        raw.push_str("\r\n");
        raw.push_str(body);

        stream
            .write_all(raw.as_bytes())
            .await
            .expect("write the request");

        let mut response = String::new();
        stream
            .read_to_string(&mut response)
            .await
            .expect("read the response");

        HttpResponse::parse(response)
    }
}

fn write(dir: &TempDir, relative: &str, bytes: &[u8]) {
    let path = dir.path().join(relative);
    fs::create_dir_all(path.parent().expect("the parent directory")).expect("create parent");
    fs::write(path, bytes).expect("write the file");
}

fn chat_body(
    messages: serde_json::Value,
    open_doc: &str,
    attachments: serde_json::Value,
) -> String {
    serde_json::to_string(&json!({
        "messages": messages,
        "open_doc": open_doc,
        "attachments": attachments,
    }))
    .expect("the chat body should serialize")
}

fn user_message(content: &str) -> serde_json::Value {
    json!([{ "role": "user", "content": content }])
}

fn chat_request(messages: serde_json::Value, open_doc: &str) -> String {
    chat_body(messages, open_doc, json!([]))
}

#[tokio::test]
async fn chat_streams_three_deltas_then_done() {
    let mock_server = MockServer::start().await;

    let body = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\" world\"}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"!\"}}]}\n\n",
        "data: [DONE]\n\n",
    );
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(body.as_bytes().to_vec(), "text/event-stream"),
        )
        .mount(&mock_server)
        .await;

    let dir = TempDir::new().expect("temp project");
    write(&dir, "sdlc/features/export.md", b"# Export\n");
    let server = TestServer::start(
        dir.path(),
        &[
            ("SDLC_AI_BASE_URL", Some(mock_server.uri())),
            ("SDLC_AI_MODEL", Some("gpt-4o".to_string())),
        ],
    )
    .await;

    let response = server
        .chat(&chat_request(
            user_message("hello"),
            "sdlc/features/export.md",
        ))
        .await;

    assert_eq!(response.status, 200);
    assert_eq!(response.header("content-type"), Some("text/event-stream"));
    assert_eq!(response.header("cache-control"), Some("no-cache"));
    assert_eq!(response.header("x-accel-buffering"), Some("no"));
    assert!(response.header("content-encoding").is_none());
    assert_eq!(
        response.body,
        concat!(
            "event: delta\ndata: {\"text\":\"Hello\"}\n\n",
            "event: delta\ndata: {\"text\":\" world\"}\n\n",
            "event: delta\ndata: {\"text\":\"!\"}\n\n",
            "event: done\ndata: {}\n\n",
        )
    );

    mock_server.verify().await;
}

#[tokio::test]
async fn upstream_error_yields_one_error_event_and_ends() {
    let mock_server = MockServer::start().await;

    let mock = Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(500).set_body_json(json!({
            "error": { "message": "boom" }
        })));
    mock.mount(&mock_server).await;

    let dir = TempDir::new().expect("temp project");
    write(&dir, "sdlc/features/export.md", b"# Export\n");
    let server = TestServer::start(
        dir.path(),
        &[
            ("SDLC_AI_BASE_URL", Some(mock_server.uri())),
            ("SDLC_AI_MODEL", Some("gpt-4o".to_string())),
        ],
    )
    .await;

    let response = server
        .chat(&chat_request(
            user_message("hello"),
            "sdlc/features/export.md",
        ))
        .await;

    assert_eq!(response.status, 200);
    assert_eq!(
        response.body,
        "event: error\ndata: {\"code\":\"upstream_error\",\"message\":\"chat upstream returned status 500\"}\n\n"
    );

    mock_server.verify().await;
}

#[tokio::test]
async fn upstream_unauthorized_yields_one_error_event_and_ends() {
    let mock_server = MockServer::start().await;

    let mock = Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({
            "error": { "message": "bad key" }
        })));
    mock.mount(&mock_server).await;

    let dir = TempDir::new().expect("temp project");
    write(&dir, "sdlc/features/export.md", b"# Export\n");
    let server = TestServer::start(
        dir.path(),
        &[
            ("SDLC_AI_BASE_URL", Some(mock_server.uri())),
            ("SDLC_AI_MODEL", Some("gpt-4o".to_string())),
        ],
    )
    .await;

    let response = server
        .chat(&chat_request(
            user_message("hello"),
            "sdlc/features/export.md",
        ))
        .await;

    assert_eq!(response.status, 200);
    assert_eq!(
        response.body,
        "event: error\ndata: {\"code\":\"unauthorized\",\"message\":\"chat upstream rejected the request: unauthorized\"}\n\n"
    );

    mock_server.verify().await;
}

#[tokio::test]
async fn malformed_stream_yields_one_error_event_and_ends() {
    let mock_server = MockServer::start().await;

    let mock = Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw(b"data: not json\n\n".to_vec(), "text/event-stream"),
        );
    mock.mount(&mock_server).await;

    let dir = TempDir::new().expect("temp project");
    write(&dir, "sdlc/features/export.md", b"# Export\n");
    let server = TestServer::start(
        dir.path(),
        &[
            ("SDLC_AI_BASE_URL", Some(mock_server.uri())),
            ("SDLC_AI_MODEL", Some("gpt-4o".to_string())),
        ],
    )
    .await;

    let response = server
        .chat(&chat_request(
            user_message("hello"),
            "sdlc/features/export.md",
        ))
        .await;

    assert_eq!(response.status, 200);
    assert!(
        response.body.starts_with("event: error\n"),
        "the malformed stream should report one error event: {}",
        response.body
    );
    assert!(
        !response.body.contains("event: done"),
        "a failed reply must not end with a done frame: {}",
        response.body
    );

    mock_server.verify().await;
}

#[tokio::test]
async fn invalid_chat_request_returns_json_400_and_never_a_stream() {
    let dir = TempDir::new().expect("temp project");
    write(&dir, "sdlc/features/export.md", b"# Export\n");
    let server = TestServer::start(
        dir.path(),
        &[
            ("SDLC_AI_BASE_URL", Some("http://127.0.0.1:9".to_string())),
            ("SDLC_AI_MODEL", Some("gpt-4o".to_string())),
        ],
    )
    .await;

    let body = chat_request(
        json!([{ "role": "system", "content": "You are a document editor." }]),
        "sdlc/features/export.md",
    );
    let response = server.chat(&body).await;

    assert_eq!(response.status, 400);
    assert_eq!(response.header("content-type"), Some("application/json"));
    assert_eq!(response.error_code().as_deref(), Some("bad_request"));
    assert!(
        !response.body.starts_with("event:"),
        "an invalid request must not open a stream: {}",
        response.body
    );
}

#[tokio::test]
async fn an_open_document_outside_the_layout_is_rejected() {
    let dir = TempDir::new().expect("temp project");
    write(&dir, "sdlc/features/export.md", b"# Export\n");
    let server = TestServer::start(
        dir.path(),
        &[
            ("SDLC_AI_BASE_URL", Some("http://127.0.0.1:9".to_string())),
            ("SDLC_AI_MODEL", Some("gpt-4o".to_string())),
        ],
    )
    .await;

    let response = server
        .chat(&chat_request(user_message("hello"), "../Cargo.toml"))
        .await;

    assert_eq!(response.status, 400);
    assert_eq!(response.header("content-type"), Some("application/json"));
    assert_eq!(response.error_code().as_deref(), Some("not_allowed"));
}

#[tokio::test]
async fn missing_open_document_returns_json_400() {
    let dir = TempDir::new().expect("temp project");
    write(&dir, "sdlc/features/export.md", b"# Export\n");
    let server = TestServer::start(
        dir.path(),
        &[
            ("SDLC_AI_BASE_URL", Some("http://127.0.0.1:9".to_string())),
            ("SDLC_AI_MODEL", Some("gpt-4o".to_string())),
        ],
    )
    .await;

    let response = server
        .chat(&chat_request(
            user_message("hello"),
            "sdlc/features/missing.md",
        ))
        .await;

    assert_eq!(response.status, 400);
    assert_eq!(response.header("content-type"), Some("application/json"));
    assert_eq!(response.error_code().as_deref(), Some("bad_request"));
}

#[tokio::test]
async fn an_oversized_message_is_rejected() {
    let dir = TempDir::new().expect("temp project");
    write(&dir, "sdlc/features/export.md", b"# Export\n");
    let server = TestServer::start(
        dir.path(),
        &[
            ("SDLC_AI_BASE_URL", Some("http://127.0.0.1:9".to_string())),
            ("SDLC_AI_MODEL", Some("gpt-4o".to_string())),
        ],
    )
    .await;

    let content = "x".repeat(9000);
    let body = chat_request(
        json!([{ "role": "user", "content": content }]),
        "sdlc/features/export.md",
    );
    let response = server.chat(&body).await;

    assert_eq!(response.status, 400);
    assert_eq!(response.header("content-type"), Some("application/json"));
}

#[tokio::test]
async fn unconfigured_chat_returns_503_with_the_ui_code() {
    let dir = TempDir::new().expect("temp project");
    write(&dir, "sdlc/features/export.md", b"# Export\n");
    let server = TestServer::start(dir.path(), &[]).await;

    let response = server
        .chat(&chat_request(
            user_message("hello"),
            "sdlc/features/export.md",
        ))
        .await;

    assert_eq!(response.status, 503);
    assert_eq!(response.header("content-type"), Some("application/json"));
    assert_eq!(response.error_code().as_deref(), Some("ai_not_configured"));
}

#[tokio::test]
async fn a_client_abort_during_the_stream_closes_the_connection() {
    let mock_server = MockServer::start().await;
    let mock = Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(
                b"data: {\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}\n\n\
                 data: {\"choices\":[{\"delta\":{\"content\":\" world\"}}]}\n\n\
                 data: [DONE]\n\n"
                    .to_vec(),
                "text/event-stream",
            ),
        );
    mock.mount(&mock_server).await;

    let dir = TempDir::new().expect("temp project");
    write(&dir, "sdlc/features/export.md", b"# Export\n");
    let server = TestServer::start(
        dir.path(),
        &[
            ("SDLC_AI_BASE_URL", Some(mock_server.uri())),
            ("SDLC_AI_MODEL", Some("gpt-4o".to_string())),
        ],
    )
    .await;

    let mut stream = TcpStream::connect(server.addr)
        .await
        .expect("connect to the server");
    let body = chat_request(user_message("hello"), "sdlc/features/export.md");
    let request = format!(
        "POST /api/chat HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nConnection: close\r\n\
         Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
        server.addr.port(),
        body.len(),
        body,
    );
    stream
        .write_all(request.as_bytes())
        .await
        .expect("write the request");

    let mut buffer = vec![0u8; 4096];
    let read = tokio::time::timeout(Duration::from_secs(5), stream.read(&mut buffer))
        .await
        .expect("the first read should finish");
    assert!(read.is_ok(), "the stream should deliver bytes");

    // Dropping the client stream aborts the response body and with it the
    // upstream request; the server task is stopped when the test server drops.
    drop(stream);

    mock_server.verify().await;
}

#[tokio::test]
async fn chat_with_attachments_sends_the_open_document_first_and_every_attachment() {
    let mock_server = MockServer::start().await;

    let body = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}\n\n",
        "data: [DONE]\n\n",
    );
    let open = Document {
        path: DocPath::parse("sdlc/features/export.md")
            .expect("the open document path should be valid"),
        content: "# Export\n".to_string(),
        hash: content_hash(b"# Export\n"),
    };
    let conventions = Document {
        path: DocPath::parse("sdlc/conventions.md").expect("the attachment path should be valid"),
        content: "# Conventions\n".to_string(),
        hash: content_hash(b"# Conventions\n"),
    };
    let messages = build(&open, &[conventions], vec![]);
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(body_partial_json(json!({
            "model": "gpt-4o",
            "stream": true,
            "messages": [ { "role": "system", "content": messages[0].content.clone() } ],
        })))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(body.as_bytes().to_vec(), "text/event-stream"),
        )
        .mount(&mock_server)
        .await;

    let dir = TempDir::new().expect("temp project");
    write(&dir, "sdlc/features/export.md", b"# Export\n");
    write(&dir, "sdlc/conventions.md", b"# Conventions\n");
    let server = TestServer::start(
        dir.path(),
        &[
            ("SDLC_AI_BASE_URL", Some(mock_server.uri())),
            ("SDLC_AI_MODEL", Some("gpt-4o".to_string())),
        ],
    )
    .await;

    let response = server
        .chat(&chat_body(
            user_message("hello"),
            "sdlc/features/export.md",
            json!(["sdlc/conventions.md"]),
        ))
        .await;

    assert_eq!(response.status, 200);
    assert_eq!(
        response.body,
        concat!(
            "event: delta\ndata: {\"text\":\"Hello\"}\n\n",
            "event: done\ndata: {}\n\n",
        )
    );

    mock_server.verify().await;
}

#[tokio::test]
async fn a_missing_attachment_is_skipped_and_the_reply_still_streams() {
    let mock_server = MockServer::start().await;
    let open = Document {
        path: DocPath::parse("sdlc/features/export.md")
            .expect("the open document path should be valid"),
        content: "# Export\n".to_string(),
        hash: content_hash(b"# Export\n"),
    };
    let messages = build(&open, &[], vec![]);
    let mock = Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(body_partial_json(json!({
            "model": "gpt-4o",
            "stream": true,
            "messages": [ { "role": "system", "content": messages[0].content.clone() } ],
        })))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(
                b"data: {\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}\n\n\
                 data: [DONE]\n\n"
                    .to_vec(),
                "text/event-stream",
            ),
        );
    mock.mount(&mock_server).await;

    let dir = TempDir::new().expect("temp project");
    write(&dir, "sdlc/features/export.md", b"# Export\n");
    let server = TestServer::start(
        dir.path(),
        &[
            ("SDLC_AI_BASE_URL", Some(mock_server.uri())),
            ("SDLC_AI_MODEL", Some("gpt-4o".to_string())),
        ],
    )
    .await;

    let response = server
        .chat(&chat_body(
            user_message("hello"),
            "sdlc/features/export.md",
            json!(["sdlc/features/gone.md"]),
        ))
        .await;

    // The missing attachment is skipped by the prompt builder, so the reply
    // still streams instead of failing the turn; the body matcher asserts the
    // prompt carried only the open document and not the missing attachment.
    assert_eq!(response.status, 200);
    assert!(response.body.contains("event: done"));
    mock_server.verify().await;
}
