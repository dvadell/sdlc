//! Integration tests for the save route (ticket 0019).

use std::fs;
use std::net::SocketAddr;
use std::path::Path;

use sdlc::config::{CliArgs, Config};
use sdlc::store::content_hash;
use sdlc::{ServerError, serve};
use serde_json::json;
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;

/// A server bound to an ephemeral loopback port, stopped when the test ends.
struct TestServer {
    addr: SocketAddr,
    handle: JoinHandle<Result<SocketAddr, ServerError>>,
}

impl TestServer {
    async fn start(dir: &Path) -> Self {
        let config = Config::from_parts(
            CliArgs {
                dir: dir.to_path_buf(),
                port: 0,
                open: false,
            },
            |_| None,
        )
        .expect("the test config should build");
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("the listener should bind");
        let addr = listener.local_addr().expect("the listener address");
        let handle = tokio::spawn(serve(config, listener));

        Self { addr, handle }
    }

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

    async fn get(&self, target: &str) -> HttpResponse {
        self.request("GET", target, None, "").await
    }

    async fn put(&self, target: &str, body: &str) -> HttpResponse {
        self.request("PUT", target, Some("application/json"), body)
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
        let headers = head
            .lines()
            .skip(1)
            .filter_map(|line| {
                let (name, value) = line.split_once(':')?;
                Some((name.trim().to_ascii_lowercase(), value.trim().to_string()))
            })
            .collect();

        Self {
            status,
            headers,
            body: body.to_string(),
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

fn write(dir: &TempDir, relative: &str, bytes: &[u8]) {
    let path = dir.path().join(relative);
    fs::create_dir_all(path.parent().expect("the parent directory")).expect("create parent");
    fs::write(path, bytes).expect("write the file");
}

#[tokio::test]
async fn save_returns_the_hash_of_the_written_bytes_and_reads_back() {
    let dir = TempDir::new().expect("temp project");
    write(&dir, "sdlc/PRD.md", b"# First\n");
    let server = TestServer::start(dir.path()).await;

    let read = server.get("/api/docs/sdlc/PRD.md").await.json();
    let old_hash = read["hash"].as_str().expect("the read hash").to_string();
    let new_content = "# Second\r\n\r\nBody with a check mark \u{2713}\n";

    let response = server
        .put(
            "/api/docs/sdlc/PRD.md",
            &json!({ "content": new_content, "expected_hash": old_hash.clone() }).to_string(),
        )
        .await;

    assert_eq!(response.status, 200);
    assert_eq!(response.header("content-type"), Some("application/json"));
    let new_hash = content_hash(new_content.as_bytes());
    assert_eq!(response.json(), json!({ "hash": new_hash }));

    let read_back = server.get("/api/docs/sdlc/PRD.md").await.json();
    assert_eq!(read_back["content"].as_str(), Some(new_content));
    assert_eq!(read_back["hash"].as_str(), Some(new_hash.as_str()));
    assert_eq!(
        fs::read(dir.path().join("sdlc/PRD.md")).expect("read the file"),
        new_content.as_bytes()
    );

    let stale = server
        .put(
            "/api/docs/sdlc/PRD.md",
            &json!({ "content": "# Third\n", "expected_hash": old_hash }).to_string(),
        )
        .await;

    assert_eq!(stale.status, 409);
    assert_eq!(stale.error_code().as_deref(), Some("conflict"));
    assert_eq!(
        fs::read(dir.path().join("sdlc/PRD.md")).expect("read the file"),
        new_content.as_bytes(),
        "a stale save must not change the file"
    );
}

#[tokio::test]
async fn an_external_write_between_read_and_save_returns_conflict() {
    let dir = TempDir::new().expect("temp project");
    write(&dir, "sdlc/PRD.md", b"# First\n");
    let server = TestServer::start(dir.path()).await;

    let read = server.get("/api/docs/sdlc/PRD.md").await.json();
    let old_hash = read["hash"].as_str().expect("the read hash").to_string();

    write(&dir, "sdlc/PRD.md", b"# External\n");

    let response = server
        .put(
            "/api/docs/sdlc/PRD.md",
            &json!({ "content": "# Save\n", "expected_hash": old_hash }).to_string(),
        )
        .await;

    assert_eq!(response.status, 409);
    assert_eq!(response.error_code().as_deref(), Some("conflict"));
    assert!(
        !response.body.contains(&content_hash(b"# External\n")),
        "the current disk hash must not be returned on conflict"
    );
    assert_eq!(
        fs::read(dir.path().join("sdlc/PRD.md")).expect("read the file"),
        b"# External\n"
    );
}

#[tokio::test]
async fn a_missing_document_returns_json_404() {
    let dir = TempDir::new().expect("temp project");
    fs::create_dir_all(dir.path().join("sdlc/features")).expect("features directory");
    let server = TestServer::start(dir.path()).await;

    let response = server
        .put(
            "/api/docs/sdlc/features/missing.md",
            &json!({
                "content": "# New\n",
                "expected_hash": content_hash(b""),
            })
            .to_string(),
        )
        .await;

    assert_eq!(response.status, 404);
    assert_eq!(response.header("content-type"), Some("application/json"));
    assert_eq!(response.error_code().as_deref(), Some("not_found"));
    assert!(!response.body.contains("/home/"));
    assert!(!dir.path().join("sdlc/features/missing.md").exists());
}

#[tokio::test]
async fn an_invalid_path_returns_json_400() {
    let dir = TempDir::new().expect("temp project");
    write(&dir, "sdlc/PRD.md", b"# PRD\n");
    let server = TestServer::start(dir.path()).await;

    for target in [
        "/api/docs/sdlc/unknown/PRD.md",
        "/api/docs/../Cargo.toml",
        "/api/docs/%2e%2e%2fCargo.toml",
    ] {
        let response = server
            .put(
                target,
                &json!({
                    "content": "# Clobber\n",
                    "expected_hash": content_hash(b"# PRD\n"),
                })
                .to_string(),
            )
            .await;

        assert_eq!(response.status, 400, "{target}");
        assert_eq!(response.header("content-type"), Some("application/json"));
        assert_eq!(
            response.error_code().as_deref(),
            Some("not_allowed"),
            "{target}"
        );
    }

    assert_eq!(
        fs::read(dir.path().join("sdlc/PRD.md")).expect("read the file"),
        b"# PRD\n"
    );
    assert!(!dir.path().join("Cargo.toml").exists());
}

#[tokio::test]
async fn an_empty_expected_hash_returns_400_before_touching_disk() {
    let dir = TempDir::new().expect("temp project");
    write(&dir, "sdlc/PRD.md", b"# Keep\n");
    fs::create_dir_all(dir.path().join("sdlc/features")).expect("features directory");
    let server = TestServer::start(dir.path()).await;

    for hash in ["", "  ", "\t\n"] {
        let response = server
            .put(
                "/api/docs/sdlc/PRD.md",
                &json!({ "content": "# Overwrite\n", "expected_hash": hash }).to_string(),
            )
            .await;

        assert_eq!(response.status, 400, "hash {hash:?}");
        assert_eq!(response.error_code().as_deref(), Some("bad_request"));
        assert_eq!(
            fs::read(dir.path().join("sdlc/PRD.md")).expect("read the file"),
            b"# Keep\n"
        );
    }

    // A blank hash is rejected before the store can report the missing file.
    let response = server
        .put(
            "/api/docs/sdlc/features/missing.md",
            &json!({ "content": "# New\n", "expected_hash": "" }).to_string(),
        )
        .await;

    assert_eq!(response.status, 400);
    assert_eq!(response.error_code().as_deref(), Some("bad_request"));
    assert!(!dir.path().join("sdlc/features/missing.md").exists());
}

#[tokio::test]
async fn an_oversized_body_returns_413_with_the_error_shape() {
    let dir = TempDir::new().expect("temp project");
    write(&dir, "sdlc/PRD.md", b"# Keep\n");
    let server = TestServer::start(dir.path()).await;

    let content = "x".repeat(3 * 1024 * 1024);
    let response = server
        .put(
            "/api/docs/sdlc/PRD.md",
            &json!({
                "content": content,
                "expected_hash": content_hash(b"# Keep\n"),
            })
            .to_string(),
        )
        .await;

    assert_eq!(response.status, 413);
    assert_eq!(response.header("content-type"), Some("application/json"));
    let json = response.json();
    assert_eq!(
        json["error"]["code"].as_str(),
        Some("payload_too_large"),
        "the oversized body must use the standard error shape"
    );
    assert!(json["error"]["message"].is_string());
    assert_eq!(
        fs::read(dir.path().join("sdlc/PRD.md")).expect("read the file"),
        b"# Keep\n"
    );
}

#[tokio::test]
async fn an_over_limit_prd_save_still_succeeds() {
    let dir = TempDir::new().expect("temp project");
    write(&dir, "sdlc/PRD.md", b"# Keep\n");
    let server = TestServer::start(dir.path()).await;

    let read = server.get("/api/docs/sdlc/PRD.md").await.json();
    let old_hash = read["hash"].as_str().expect("the read hash").to_string();
    let content = format!("{}\n", "x".repeat(12_000));

    // The warnings are advisory and never block a save, so a document that grew
    // past the PRD size limit still saves (ticket 0057).
    let response = server
        .put(
            "/api/docs/sdlc/PRD.md",
            &json!({ "content": content, "expected_hash": old_hash }).to_string(),
        )
        .await;

    assert_eq!(response.status, 200);
    assert_eq!(
        fs::read(dir.path().join("sdlc/PRD.md")).expect("read the file"),
        content.as_bytes()
    );
}

#[tokio::test]
async fn a_non_json_content_type_returns_415_from_the_guard() {
    let dir = TempDir::new().expect("temp project");
    write(&dir, "sdlc/PRD.md", b"# Keep\n");
    let server = TestServer::start(dir.path()).await;

    let response = server
        .request(
            "PUT",
            "/api/docs/sdlc/PRD.md",
            Some("text/plain"),
            &json!({
                "content": "# Overwrite\n",
                "expected_hash": content_hash(b"# Keep\n"),
            })
            .to_string(),
        )
        .await;

    assert_eq!(response.status, 415);
    assert_eq!(response.header("content-type"), Some("application/json"));
    assert_eq!(
        response.error_code().as_deref(),
        Some("unsupported_media_type")
    );
    assert_eq!(
        fs::read(dir.path().join("sdlc/PRD.md")).expect("read the file"),
        b"# Keep\n"
    );
}
