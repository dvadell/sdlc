//! Integration tests for the Host/Origin guard middleware (ticket 0006).

use std::net::SocketAddr;
use std::path::PathBuf;

use sdlc::config::{CliArgs, Config};
use sdlc::{ServerError, serve};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;

/// A server bound to an ephemeral loopback port, stopped when the test ends.
struct TestServer {
    addr: SocketAddr,
    handle: JoinHandle<Result<SocketAddr, ServerError>>,
}

impl TestServer {
    async fn start() -> Self {
        let config = Config::from_parts(
            CliArgs {
                dir: PathBuf::from("/tmp"),
                port: 0,
                open: false,
            },
            |_| None,
        )
        .expect("guard test config should build");

        let listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("the listener should bind");
        let addr = listener.local_addr().expect("the listener address");
        let handle = tokio::spawn(serve(config, listener));

        Self { addr, handle }
    }

    fn port(&self) -> u16 {
        self.addr.port()
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

struct TestResponse {
    status: u16,
    body: String,
}

impl TestResponse {
    fn error_code(&self) -> Option<String> {
        let json: serde_json::Value =
            serde_json::from_str(&self.body).expect("the response body should be JSON");
        json["error"]["code"].as_str().map(str::to_owned)
    }
}

/// Sends a raw HTTP/1.1 request with exactly the supplied headers.
async fn request(
    addr: SocketAddr,
    method: &str,
    target: &str,
    headers: &[(&str, &str)],
    body: &str,
) -> TestResponse {
    let mut stream = TcpStream::connect(addr)
        .await
        .expect("connect to the server");

    let mut raw = format!("{method} {target} HTTP/1.1\r\n");
    for (name, value) in headers {
        raw.push_str(name);
        raw.push_str(": ");
        raw.push_str(value);
        raw.push_str("\r\n");
    }
    if !body.is_empty() {
        raw.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    raw.push_str("Connection: close\r\n\r\n");
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

    let status = response
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse::<u16>().ok())
        .expect("the status line should carry a numeric status");
    let (_, body) = response
        .split_once("\r\n\r\n")
        .expect("the response should have a head and a body");

    TestResponse {
        status,
        body: body.trim().to_string(),
    }
}

fn assert_forbidden(response: &TestResponse) {
    assert_eq!(response.status, 403);
    assert_eq!(response.error_code().as_deref(), Some("forbidden"));
}

fn assert_unsupported_media_type(response: &TestResponse) {
    assert_eq!(response.status, 415);
    assert_eq!(
        response.error_code().as_deref(),
        Some("unsupported_media_type")
    );
}

#[tokio::test]
async fn same_origin_host_and_origin_pass() {
    let server = TestServer::start().await;
    let port = server.port();
    let host = format!("127.0.0.1:{port}");
    let origin = format!("http://127.0.0.1:{port}");

    let response = request(
        server.addr,
        "GET",
        "/",
        &[("Host", &host), ("Origin", &origin)],
        "",
    )
    .await;

    assert_eq!(response.status, 200);
}

#[tokio::test]
async fn localhost_and_loopback_are_both_accepted() {
    let server = TestServer::start().await;
    let port = server.port();

    for host_name in ["127.0.0.1", "localhost"] {
        let host = format!("{host_name}:{port}");
        let origin = format!("http://{host_name}:{port}");
        let response = request(
            server.addr,
            "GET",
            "/",
            &[("Host", &host), ("Origin", &origin)],
            "",
        )
        .await;

        assert_eq!(response.status, 200, "host {host_name} should be accepted");
    }
}

#[tokio::test]
async fn absent_origin_passes() {
    let server = TestServer::start().await;
    let host = format!("127.0.0.1:{}", server.port());

    let response = request(server.addr, "GET", "/", &[("Host", &host)], "").await;

    assert_eq!(response.status, 200);
}

#[tokio::test]
async fn get_without_content_type_passes() {
    let server = TestServer::start().await;
    let host = format!("127.0.0.1:{}", server.port());

    let response = request(server.addr, "GET", "/", &[("Host", &host)], "").await;

    assert_eq!(response.status, 200);
}

#[tokio::test]
async fn foreign_host_is_rejected() {
    let server = TestServer::start().await;

    let response = request(server.addr, "GET", "/", &[("Host", "evil.example")], "").await;

    assert_forbidden(&response);
}

#[tokio::test]
async fn host_with_the_wrong_port_is_rejected() {
    let server = TestServer::start().await;
    let host = format!("127.0.0.1:{}", server.port() + 1);

    let response = request(server.addr, "GET", "/", &[("Host", &host)], "").await;

    assert_forbidden(&response);
}

#[tokio::test]
async fn missing_host_is_rejected() {
    let server = TestServer::start().await;

    let response = request(server.addr, "GET", "/", &[], "").await;

    assert_forbidden(&response);
}

#[tokio::test]
async fn unparsable_host_is_rejected() {
    let server = TestServer::start().await;

    let response = request(server.addr, "GET", "/", &[("Host", "not a host")], "").await;

    assert_forbidden(&response);
}

#[tokio::test]
async fn two_host_headers_are_rejected() {
    let server = TestServer::start().await;
    let host = format!("127.0.0.1:{}", server.port());

    let response = request(
        server.addr,
        "GET",
        "/",
        &[("Host", &host), ("Host", &host)],
        "",
    )
    .await;

    assert_forbidden(&response);
}

#[tokio::test]
async fn foreign_origin_is_rejected() {
    let server = TestServer::start().await;
    let host = format!("127.0.0.1:{}", server.port());

    let response = request(
        server.addr,
        "GET",
        "/",
        &[("Host", &host), ("Origin", "http://evil.test")],
        "",
    )
    .await;

    assert_forbidden(&response);
}

#[tokio::test]
async fn https_origin_is_rejected() {
    let server = TestServer::start().await;
    let host = format!("127.0.0.1:{}", server.port());
    let origin = format!("https://127.0.0.1:{}", server.port());

    let response = request(
        server.addr,
        "GET",
        "/",
        &[("Host", &host), ("Origin", &origin)],
        "",
    )
    .await;

    assert_forbidden(&response);
}

#[tokio::test]
async fn null_origin_is_rejected() {
    let server = TestServer::start().await;
    let host = format!("127.0.0.1:{}", server.port());

    let response = request(
        server.addr,
        "GET",
        "/",
        &[("Host", &host), ("Origin", "null")],
        "",
    )
    .await;

    assert_forbidden(&response);
}

#[tokio::test]
async fn two_origin_headers_are_rejected() {
    let server = TestServer::start().await;
    let host = format!("127.0.0.1:{}", server.port());
    let origin = format!("http://127.0.0.1:{}", server.port());

    let response = request(
        server.addr,
        "GET",
        "/",
        &[("Host", &host), ("Origin", &origin), ("Origin", &origin)],
        "",
    )
    .await;

    assert_forbidden(&response);
}

#[tokio::test]
async fn post_with_text_plain_is_rejected_before_the_handler() {
    let server = TestServer::start().await;
    let host = format!("127.0.0.1:{}", server.port());

    let response = request(
        server.addr,
        "POST",
        "/api/unknown",
        &[("Host", &host), ("Content-Type", "text/plain")],
        "{}",
    )
    .await;

    assert_unsupported_media_type(&response);
}

#[tokio::test]
async fn post_without_content_type_is_rejected() {
    let server = TestServer::start().await;
    let host = format!("127.0.0.1:{}", server.port());

    let response = request(server.addr, "POST", "/api/unknown", &[("Host", &host)], "").await;

    assert_unsupported_media_type(&response);
}

#[tokio::test]
async fn state_changing_methods_require_json() {
    let server = TestServer::start().await;
    let host = format!("127.0.0.1:{}", server.port());

    for method in ["PUT", "PATCH", "DELETE"] {
        let response = request(
            server.addr,
            method,
            "/api/unknown",
            &[("Host", &host), ("Content-Type", "text/plain")],
            "",
        )
        .await;

        assert_unsupported_media_type(&response);
    }
}

#[tokio::test]
async fn post_with_json_content_type_reaches_the_handler() {
    let server = TestServer::start().await;
    let host = format!("127.0.0.1:{}", server.port());

    let response = request(
        server.addr,
        "POST",
        "/api/unknown",
        &[
            ("Host", &host),
            ("Content-Type", "application/json; charset=utf-8"),
        ],
        "",
    )
    .await;

    // The guard passed; the asset fallback rejects POST with 405.
    assert_eq!(response.status, 405);
}

#[tokio::test]
async fn empty_json_body_is_allowed() {
    let server = TestServer::start().await;
    let host = format!("127.0.0.1:{}", server.port());

    let response = request(
        server.addr,
        "POST",
        "/api/unknown",
        &[("Host", &host), ("Content-Type", "application/json")],
        "",
    )
    .await;

    // The guard passed; the asset fallback rejects POST with 405.
    assert_eq!(response.status, 405);
}

#[tokio::test]
async fn header_values_are_case_insensitive_where_http_requires_it() {
    let server = TestServer::start().await;
    let host = format!("LOCALHOST:{}", server.port());
    let origin = format!("HTTP://LocalHost:{}", server.port());

    let response = request(
        server.addr,
        "POST",
        "/api/unknown",
        &[
            ("hOsT", &host),
            ("oRiGiN", &origin),
            ("cOnTeNt-TyPe", "Application/JSON; Charset=UTF-8"),
        ],
        "",
    )
    .await;

    // The guard passed; the asset fallback rejects POST with 405.
    assert_eq!(response.status, 405);
}
