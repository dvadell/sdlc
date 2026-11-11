//! Integration tests for the chat SSE route (ticket 0036).

use std::fs;
use std::net::SocketAddr;
use std::path::Path;
use std::time::Duration;

use sdlc::chat::{ExistingTicket, TicketContext, build};
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

fn chat_body_with_ticket_context(
    messages: serde_json::Value,
    open_doc: &str,
    attachments: serde_json::Value,
    ticket_context: serde_json::Value,
) -> String {
    let mut body = serde_json::Map::new();
    body.insert("messages".to_string(), messages);
    body.insert("open_doc".to_string(), json!(open_doc));
    body.insert("attachments".to_string(), attachments);
    body.insert("ticket_context".to_string(), ticket_context);
    serde_json::to_string(&serde_json::Value::Object(body)).expect("the chat body should serialize")
}

fn user_message(content: &str) -> serde_json::Value {
    json!([{ "role": "user", "content": content }])
}

fn chat_request(messages: serde_json::Value, open_doc: &str) -> String {
    chat_body(messages, open_doc, json!([]))
}

fn chat_request_with_ticket_context(
    messages: serde_json::Value,
    open_doc: &str,
    ticket_context: serde_json::Value,
) -> String {
    chat_body_with_ticket_context(messages, open_doc, json!([]), ticket_context)
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
    let messages = build(&open, &[conventions], vec![], None);
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
    let messages = build(&open, &[], vec![], None);
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

/// The SSE frame of one chat event, found in the body of a reply.
///
/// The replies under test carry at most one frame per event name, so the first
/// frame carrying the requested event name is the frame under test; the
/// contract is single-match.
fn frame_of<'a>(body: &'a str, event: &str) -> &'a str {
    let prefix = format!("event: {event}");
    body.split("\n\n")
        .map(str::trim)
        .find(|frame| frame.starts_with(prefix.as_str()))
        .unwrap_or_else(|| panic!("the {event} frame should be in the body: {body}"))
}

/// The JSON data payload of one SSE frame.
fn frame_data(frame: &str) -> serde_json::Value {
    let mut lines = frame.split('\n');
    let _event = lines.next().unwrap_or("");
    let data = lines
        .next()
        .unwrap_or("")
        .strip_prefix("data: ")
        .unwrap_or_else(|| panic!("the frame should carry data: {frame}"));
    serde_json::from_str(data).expect("the frame data should be JSON")
}

#[tokio::test]
async fn a_clean_reply_with_a_file_block_emits_delta_proposal_and_done() {
    let mock_server = MockServer::start().await;

    let body = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"I will add the export.\"}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"\\n=== FILE: sdlc/features/export.md ===\\n# Export v2\\n=== END FILE ===\"}}]}\n\n",
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
            user_message("add the export"),
            "sdlc/features/export.md",
        ))
        .await;

    assert_eq!(response.status, 200);
    assert_eq!(response.header("content-type"), Some("text/event-stream"));
    assert!(
        response
            .body
            .starts_with("event: delta\ndata: {\"text\":\"I will add the export.\"}\n\n"),
        "the reply text should stream first: {}",
        response.body
    );
    assert!(
        response.body.ends_with("event: done\ndata: {}\n\n"),
        "the clean reply should end with the done frame: {}",
        response.body
    );

    let delta_at = response
        .body
        .find("event: delta")
        .expect("the delta frame should be in the body");
    let proposal_at = response
        .body
        .find("event: proposal")
        .expect("the proposal frame should be in the body");
    let done_at = response
        .body
        .find("event: done")
        .expect("the done frame should be in the body");
    assert!(
        delta_at < proposal_at && proposal_at < done_at,
        "the frames should stream delta, proposal, done: {}",
        response.body
    );

    let proposal = frame_data(frame_of(&response.body, "proposal"));
    assert!(!proposal["id"].as_str().expect("the proposal id").is_empty());
    let changes = proposal["changes"]
        .as_array()
        .expect("the proposal changes");
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0]["path"], "sdlc/features/export.md");
    assert_eq!(changes[0]["action"], "modify");
    assert!(
        changes[0]["diff"]
            .as_str()
            .expect("the proposal diff should be a string")
            .contains("--- a/sdlc/features/export.md"),
        "the proposal diff should show the change: {}",
        changes[0]["diff"]
    );
    assert_eq!(
        proposal["warnings"]
            .as_array()
            .expect("the proposal warnings")
            .len(),
        0
    );

    // The reply only built the proposal in memory: no file was written here.
    assert_eq!(
        fs::read_to_string(dir.path().join("sdlc/features/export.md")).expect("read the feature"),
        "# Export\n"
    );

    mock_server.verify().await;
}

#[tokio::test]
async fn a_reply_with_a_write_and_a_delete_emits_one_proposal_with_both_changes() {
    let mock_server = MockServer::start().await;

    let body = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"I will update the export and retire the old ticket.\"}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"\\n=== FILE: sdlc/features/export.md ===\\n# Export v2\\n=== END FILE ===\"}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"\\n=== DELETE: sdlc/epics/0001-foundation/tickets/0007-old-ticket.md ===\"}}]}\n\n",
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
    write(
        &dir,
        "sdlc/epics/0001-foundation/tickets/0007-old-ticket.md",
        b"# Old Ticket\n",
    );
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
            user_message("update the export and retire the old ticket"),
            "sdlc/features/export.md",
        ))
        .await;

    assert_eq!(response.status, 200);
    let delta_at = response
        .body
        .find("event: delta")
        .expect("the delta frame should be in the body");
    let proposal_at = response
        .body
        .find("event: proposal")
        .expect("the proposal frame should be in the body");
    let done_at = response
        .body
        .find("event: done")
        .expect("the done frame should be in the body");
    assert!(
        delta_at < proposal_at && proposal_at < done_at,
        "the frames should stream delta, proposal, done: {}",
        response.body
    );
    assert!(
        !response.body.contains("event: warning"),
        "a clean reply should not warn: {}",
        response.body
    );
    assert!(response.body.ends_with("event: done\ndata: {}\n\n"));

    let proposal = frame_data(frame_of(&response.body, "proposal"));
    let changes = proposal["changes"]
        .as_array()
        .expect("the proposal changes");
    assert_eq!(changes.len(), 2);

    assert_eq!(changes[0]["path"], "sdlc/features/export.md");
    assert_eq!(changes[0]["action"], "modify");
    assert!(
        changes[0]["diff"]
            .as_str()
            .expect("the proposal diff should be a string")
            .contains("--- a/sdlc/features/export.md")
    );

    assert_eq!(
        changes[1]["path"],
        "sdlc/epics/0001-foundation/tickets/0007-old-ticket.md"
    );
    assert_eq!(changes[1]["action"], "delete");

    mock_server.verify().await;
}

#[tokio::test]
async fn an_unterminated_block_emits_a_warning_then_done_and_no_proposal() {
    let mock_server = MockServer::start().await;

    let body = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"Sure, I will update it.\"}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"\\n=== FILE: sdlc/features/export.md ===\\n# No end marker\"}}]}\n\n",
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
            user_message("update it"),
            "sdlc/features/export.md",
        ))
        .await;

    assert_eq!(response.status, 200);
    assert!(response.body.contains("event: warning"));
    assert_eq!(
        frame_data(frame_of(&response.body, "warning"))["message"],
        "unterminated FILE block for sdlc/features/export.md: missing end marker"
    );
    assert!(
        !response.body.contains("event: proposal"),
        "a malformed block must not become a proposal: {}",
        response.body
    );
    assert!(
        response.body.ends_with("event: done\ndata: {}\n\n"),
        "the malformed reply should end with the done frame: {}",
        response.body
    );

    mock_server.verify().await;
}

#[tokio::test]
async fn an_invalid_proposed_path_emits_an_error_and_no_proposal_and_writes_nothing() {
    let mock_server = MockServer::start().await;

    let body = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"I will edit the config.\"}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"\\n=== FILE: ../../etc/passwd ===\\nroot:x:0:0\\n=== END FILE ===\"}}]}\n\n",
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
            user_message("edit the config"),
            "sdlc/features/export.md",
        ))
        .await;

    assert_eq!(response.status, 200);
    assert!(
        response.body.contains("event: delta"),
        "the streamed delta should still be relayed: {}",
        response.body
    );
    let error_frame = frame_of(&response.body, "error");
    assert_eq!(frame_data(error_frame)["code"], json!("invalid_path"));
    assert_eq!(
        frame_data(error_frame)["message"],
        json!("the proposed path is not an allowed document: ../../etc/passwd")
    );
    assert!(
        !response.body.contains("event: done"),
        "an error must not be followed by a done frame: {}",
        response.body
    );

    assert_eq!(
        fs::read_to_string(dir.path().join("sdlc/features/export.md")).expect("read the feature"),
        "# Export\n",
        "a refused proposal must not write any file"
    );

    mock_server.verify().await;
}

#[tokio::test]
async fn a_reply_proposing_an_unmanaged_rules_file_fails_and_writes_nothing() {
    let mock_server = MockServer::start().await;

    let body = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"I will add the rules file.\"}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"\\n=== FILE: CLAUDE.md ===\\n# Rules\\n=== END FILE ===\"}}]}\n\n",
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
    write(&dir, "AGENTS.md", b"# Agent rules\n");
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
            user_message("add the rules file"),
            "AGENTS.md",
        ))
        .await;

    assert_eq!(response.status, 200);
    assert!(
        response.body.contains("event: delta"),
        "the streamed delta should still be relayed: {}",
        response.body
    );
    let error_frame = frame_of(&response.body, "error");
    assert_eq!(frame_data(error_frame)["code"], json!("invalid_path"));
    assert_eq!(
        frame_data(error_frame)["message"],
        json!("the proposed path is not an allowed document: CLAUDE.md")
    );
    assert!(
        !response.body.contains("event: done"),
        "an error must not be followed by a done frame: {}",
        response.body
    );
    assert!(
        !response.body.contains("event: proposal"),
        "a refused proposal must not be surfaced: {}",
        response.body
    );

    assert_eq!(
        fs::read_to_string(dir.path().join("AGENTS.md")).expect("read the agents"),
        "# Agent rules\n",
        "a refused proposal must not write any file"
    );

    mock_server.verify().await;
}

#[tokio::test]
async fn a_truncated_upstream_reply_emits_no_proposal() {
    let mock_server = MockServer::start().await;

    let body = "data: {\"choices\":[{\"delta\":{\"content\":\"I will add\"}}]}\n\n";
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
    assert!(
        response.body.contains("event: delta"),
        "the partial delta should still be relayed: {}",
        response.body
    );
    let error_frame = frame_of(&response.body, "error");
    assert_eq!(frame_data(error_frame)["code"], json!("truncated"));
    assert_eq!(
        frame_data(error_frame)["message"],
        json!("chat upstream stream ended before the reply was complete")
    );
    assert!(
        !response.body.contains("event: proposal"),
        "a truncated reply must not become a proposal: {}",
        response.body
    );

    mock_server.verify().await;
}

#[tokio::test]
async fn an_ordinary_reply_without_blocks_emits_no_proposal() {
    let mock_server = MockServer::start().await;

    let body = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"Just an ordinary answer.\"}}]}\n\n",
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
    assert_eq!(
        response.body,
        concat!(
            "event: delta\ndata: {\"text\":\"Just an ordinary answer.\"}\n\n",
            "event: done\ndata: {}\n\n",
        )
    );

    mock_server.verify().await;
}

#[tokio::test]
async fn a_reply_that_modifies_and_deletes_an_existing_ticket_emits_the_regeneration_proposal() {
    let mock_server = MockServer::start().await;

    let body = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"I will update the export ticket and retire the import ticket.\"}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"\\n=== FILE: sdlc/epics/0001-foundation/tickets/0001-add-export.md ===\\n---\\nstatus: in-progress\\nfeature: export\\n---\\n\\n# 0001: Add export\\n\\n## Description\\n\\nAdd the export endpoint.\\n=== END FILE ===\"}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"\\n=== DELETE: sdlc/epics/0001-foundation/tickets/0002-add-import.md ===\"}}]}\n\n",
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
    write(
        &dir,
        "sdlc/epics/0001-foundation/description.md",
        b"---\nstatus: planned\n---\n\n# 0001: Foundation\n",
    );
    write(
        &dir,
        "sdlc/epics/0001-foundation/tickets/0001-add-export.md",
        b"---\nstatus: todo\nfeature: export\n---\n\n# 0001: Add export\n\n## Description\n\nAdd the export endpoint.\n",
    );
    write(
        &dir,
        "sdlc/epics/0001-foundation/tickets/0002-add-import.md",
        b"---\nstatus: todo\nfeature: export\n---\n\n# 0002: Add import\n\n## Description\n\nAdd the import endpoint.\n",
    );
    let server = TestServer::start(
        dir.path(),
        &[
            ("SDLC_AI_BASE_URL", Some(mock_server.uri())),
            ("SDLC_AI_MODEL", Some("gpt-4o".to_string())),
        ],
    )
    .await;

    let response = server
        .chat(&chat_request_with_ticket_context(
            user_message("update the tickets"),
            "sdlc/features/export.md",
            json!({ "feature": "export", "epic": "0001-foundation" }),
        ))
        .await;

    assert_eq!(response.status, 200);
    assert!(
        response.body.ends_with("event: done\ndata: {}\n\n"),
        "the regeneration reply should end with the done frame: {}",
        response.body
    );

    let delta_at = response
        .body
        .find("event: delta")
        .expect("the delta frame should be in the body");
    let proposal_at = response
        .body
        .find("event: proposal")
        .expect("the proposal frame should be in the body");
    let done_at = response
        .body
        .find("event: done")
        .expect("the done frame should be in the body");
    assert!(
        delta_at < proposal_at && proposal_at < done_at,
        "the frames should stream delta, proposal, done: {}",
        response.body
    );

    let proposal = frame_data(frame_of(&response.body, "proposal"));
    let changes = proposal["changes"]
        .as_array()
        .expect("the proposal changes");
    assert_eq!(changes.len(), 2);

    // The modified existing ticket keeps its numbered path and becomes a
    // modify carrying the new content in its diff.
    assert_eq!(
        changes[0]["path"],
        "sdlc/epics/0001-foundation/tickets/0001-add-export.md"
    );
    assert_eq!(changes[0]["action"], "modify");
    assert!(
        changes[0]["diff"]
            .as_str()
            .expect("the proposal diff should be a string")
            .contains("+status: in-progress"),
        "the proposal diff should carry the new content of the ticket: {}",
        changes[0]["diff"]
    );

    // The deleted existing ticket becomes a delete with the old content in
    // its diff.
    assert_eq!(
        changes[1]["path"],
        "sdlc/epics/0001-foundation/tickets/0002-add-import.md"
    );
    assert_eq!(changes[1]["action"], "delete");
    assert!(
        changes[1]["diff"]
            .as_str()
            .expect("the proposal diff should be a string")
            .contains("Add the import endpoint"),
        "the proposal diff should carry the old content of the ticket: {}",
        changes[1]["diff"]
    );

    let warnings = proposal["warnings"]
        .as_array()
        .expect("the proposal warnings");
    assert_eq!(warnings.len(), 1);
    assert_eq!(
        warnings[0],
        json!("2 existing tickets included"),
        "the proposal should note how many tickets were included"
    );

    mock_server.verify().await;
}

#[tokio::test]
async fn a_ticket_generation_request_includes_the_existing_tickets_of_the_selected_epic() {
    let mock_server = MockServer::start().await;

    let open = Document {
        path: DocPath::parse("sdlc/features/export.md")
            .expect("the open document path should be valid"),
        content: "# Export\n".to_string(),
        hash: content_hash(b"# Export\n"),
    };
    let ticket_context = TicketContext {
        feature: Some("export".to_string()),
        epic: Some("0001-foundation".to_string()),
        existing: vec![
            ExistingTicket {
                path: "sdlc/epics/0001-foundation/tickets/0001-add-export.md".to_string(),
                content: "---\nstatus: todo\nfeature: export\n---\n\n# 0001: Add export\n"
                    .to_string(),
            },
            ExistingTicket {
                path: "sdlc/epics/0001-foundation/tickets/0002-add-import.md".to_string(),
                content: "---\nstatus: todo\nfeature: export\n---\n\n# 0002: Add import\n"
                    .to_string(),
            },
        ],
    };
    let messages = build(&open, &[], vec![], Some(&ticket_context));
    let body = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"I will draft the export tickets.\"}}]}\n\n",
        "data: [DONE]\n\n",
    );
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
    write(
        &dir,
        "sdlc/epics/0001-foundation/description.md",
        b"---\nstatus: planned\n---\n\n# 0001: Foundation\n",
    );
    write(
        &dir,
        "sdlc/epics/0001-foundation/tickets/0001-add-export.md",
        b"---\nstatus: todo\nfeature: export\n---\n\n# 0001: Add export\n",
    );
    write(
        &dir,
        "sdlc/epics/0001-foundation/tickets/0002-add-import.md",
        b"---\nstatus: todo\nfeature: export\n---\n\n# 0002: Add import\n",
    );
    // The ticket of another feature is in the selected epic but must not be
    // part of the regeneration context.
    write(
        &dir,
        "sdlc/epics/0001-foundation/tickets/0003-add-audit.md",
        b"---\nstatus: todo\nfeature: other\n---\n\n# 0003: Add audit\n",
    );
    let server = TestServer::start(
        dir.path(),
        &[
            ("SDLC_AI_BASE_URL", Some(mock_server.uri())),
            ("SDLC_AI_MODEL", Some("gpt-4o".to_string())),
        ],
    )
    .await;

    let response = server
        .chat(&chat_request_with_ticket_context(
            user_message("hello"),
            "sdlc/features/export.md",
            json!({ "feature": "export", "epic": "0001-foundation" }),
        ))
        .await;

    assert_eq!(response.status, 200);
    assert!(response.body.contains("event: done"));
    mock_server.verify().await;
}
