//! Integration tests for the proposal accept and reject routes (ticket 0043).

use std::fs;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;

use axum::serve;
use sdlc::chat::Block;
use sdlc::config::{CliArgs, Config};
use sdlc::proposal::ProposalService;
use sdlc::router;
use sdlc::state::AppState;
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;

/// A server bound to an ephemeral loopback port, serving the real `/api` router
/// with the proposal service of one project directory.
struct TestServer {
    addr: SocketAddr,
    handle: JoinHandle<()>,
    proposals: Arc<ProposalService>,
}

impl TestServer {
    async fn start(dir: &Path) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("the listener should bind");
        let addr = listener.local_addr().expect("the listener address");
        let config = Config::from_parts(
            CliArgs {
                dir: dir.to_path_buf(),
                port: 0,
                open: false,
            },
            |_| None,
        )
        .expect("the test config should build");
        let state = AppState::new(config, addr.port()).expect("the state should build");
        let proposals = state.proposals.clone();
        let handle = tokio::spawn(async move {
            let _ = serve(listener, router(state))
                .with_graceful_shutdown(async {
                    let _ = tokio::signal::ctrl_c().await;
                })
                .await;
        });

        Self {
            addr,
            handle,
            proposals,
        }
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
        if !body.is_empty() {
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

    async fn post(&self, target: &str, body: &str) -> HttpResponse {
        self.request("POST", target, Some("application/json"), body)
            .await
    }

    /// Seeds a proposal through the public proposal service of the server.
    async fn seed(&self, blocks: &[Block]) -> String {
        let proposal = self
            .proposals
            .create(blocks)
            .await
            .expect("the proposal should build");

        proposal.id
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

    fn json(&self) -> Value {
        serde_json::from_str(&self.body).expect("the response body should be JSON")
    }

    fn error_code(&self) -> Option<String> {
        self.json()["error"]["code"].as_str().map(str::to_owned)
    }
}

fn assert_error(response: &HttpResponse, status: u16, code: &str) {
    assert_eq!(response.status, status, "body: {}", response.body);
    assert_eq!(response.header("content-type"), Some("application/json"));
    assert_eq!(response.error_code().as_deref(), Some(code));
}

fn write(dir: &TempDir, relative: &str, content: &str) {
    let path = dir.path().join(relative);
    fs::create_dir_all(path.parent().expect("the parent directory")).expect("create parent");
    fs::write(path, content).expect("write the file");
}

#[tokio::test]
async fn accept_applies_the_proposal_and_returns_the_applied_paths() {
    let dir = TempDir::new().expect("temp project");
    let server = TestServer::start(dir.path()).await;
    write(&dir, "sdlc/features/export.md", "# Export\n");
    let id = server
        .seed(&[Block::Write {
            path: "sdlc/features/export.md".to_string(),
            content: "# Exported\n".to_string(),
        }])
        .await;

    let response = server
        .post(&format!("/api/proposals/{id}/accept"), "{}")
        .await;

    assert_eq!(response.status, 200);
    assert_eq!(response.header("content-type"), Some("application/json"));
    let body = response.json();
    assert_eq!(body["applied"], json!(["sdlc/features/export.md"]));
    assert!(
        body.get("error").is_none(),
        "a successful accept carries no error: {}",
        response.body
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("sdlc/features/export.md")).expect("read the feature"),
        "# Exported\n"
    );
}

#[tokio::test]
async fn reject_removes_the_proposal_and_answers_rejected_true() {
    let dir = TempDir::new().expect("temp project");
    let server = TestServer::start(dir.path()).await;
    write(&dir, "sdlc/features/export.md", "# Export\n");
    let id = server
        .seed(&[Block::Write {
            path: "sdlc/features/export.md".to_string(),
            content: "# Exported\n".to_string(),
        }])
        .await;

    let response = server
        .post(&format!("/api/proposals/{id}/reject"), "{}")
        .await;

    assert_eq!(response.status, 200);
    assert_eq!(response.header("content-type"), Some("application/json"));
    assert_eq!(response.json(), json!({ "rejected": true }));

    // A rejected proposal is gone, so the accept of the same id is unknown.
    let response = server
        .post(&format!("/api/proposals/{id}/accept"), "{}")
        .await;

    assert_error(&response, 404, "not_found");
}

#[tokio::test]
async fn an_unknown_proposal_id_is_not_found_for_accept_and_reject() {
    let dir = TempDir::new().expect("temp project");
    let server = TestServer::start(dir.path()).await;

    let response = server
        .post("/api/proposals/no-such-proposal/accept", "{}")
        .await;
    assert_error(&response, 404, "not_found");

    let response = server
        .post("/api/proposals/no-such-proposal/reject", "{}")
        .await;
    assert_error(&response, 404, "not_found");
}

#[tokio::test]
async fn accepting_a_proposal_twice_answers_not_found_the_second_time() {
    let dir = TempDir::new().expect("temp project");
    let server = TestServer::start(dir.path()).await;
    write(&dir, "sdlc/features/export.md", "# Export\n");
    let id = server
        .seed(&[Block::Write {
            path: "sdlc/features/export.md".to_string(),
            content: "# Exported\n".to_string(),
        }])
        .await;

    let first = server
        .post(&format!("/api/proposals/{id}/accept"), "{}")
        .await;
    assert_eq!(first.status, 200);

    let second = server
        .post(&format!("/api/proposals/{id}/accept"), "{}")
        .await;
    assert_error(&second, 404, "not_found");
}

#[tokio::test]
async fn accepting_a_stale_proposal_answers_conflict_and_writes_nothing() {
    let dir = TempDir::new().expect("temp project");
    let server = TestServer::start(dir.path()).await;
    write(&dir, "sdlc/features/export.md", "# Export\n");
    let id = server
        .seed(&[Block::Write {
            path: "sdlc/features/export.md".to_string(),
            content: "# Exported\n".to_string(),
        }])
        .await;

    // The person changed the file after the proposal was built.
    fs::write(
        dir.path().join("sdlc/features/export.md"),
        "# Changed externally\n",
    )
    .expect("rewrite the feature");

    let response = server
        .post(&format!("/api/proposals/{id}/accept"), "{}")
        .await;

    assert_error(&response, 409, "conflict");
    assert_eq!(
        fs::read_to_string(dir.path().join("sdlc/features/export.md")).expect("read the feature"),
        "# Changed externally\n"
    );
}

#[tokio::test]
async fn a_partial_apply_answers_500_with_the_applied_and_failed_paths() {
    let dir = TempDir::new().expect("temp project");
    let server = TestServer::start(dir.path()).await;
    write(&dir, "sdlc/PRD.md", "# Product Requirements Document\n");
    let id = server
        .seed(&[
            Block::Write {
                path: "sdlc/PRD.md".to_string(),
                content: "# Fresh PRD\n".to_string(),
            },
            // The `sdlc/features/` directory does not exist yet, so the create
            // of the second change fails while the first one was applied.
            Block::Write {
                path: "sdlc/features/gamma.md".to_string(),
                content: "# Gamma\n".to_string(),
            },
        ])
        .await;

    let response = server
        .post(&format!("/api/proposals/{id}/accept"), "{}")
        .await;

    assert_eq!(response.status, 500);
    assert_eq!(response.header("content-type"), Some("application/json"));
    let body = response.json();
    assert_eq!(body["error"]["code"], json!("partial_apply"));
    assert_eq!(body["applied"], json!(["sdlc/PRD.md"]));
    assert_eq!(body["failed"], json!("sdlc/features/gamma.md"));
}

#[tokio::test]
async fn a_missing_content_type_is_rejected_with_415_by_the_guard() {
    let dir = TempDir::new().expect("temp project");
    let server = TestServer::start(dir.path()).await;
    write(&dir, "sdlc/features/export.md", "# Export\n");
    let id = server
        .seed(&[Block::Write {
            path: "sdlc/features/export.md".to_string(),
            content: "# Exported\n".to_string(),
        }])
        .await;

    let response = server
        .request("POST", &format!("/api/proposals/{id}/accept"), None, "{}")
        .await;

    assert_error(&response, 415, "unsupported_media_type");
}
