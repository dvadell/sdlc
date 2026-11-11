//! Integration tests for the read routes (ticket 0018).

use std::fs;
use std::net::SocketAddr;
use std::path::Path;

use sdlc::config::{CliArgs, Config};
use sdlc::store::content_hash;
use sdlc::{ServerError, serve};
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

    async fn request(&self, method: &str, target: &str) -> HttpResponse {
        let mut stream = TcpStream::connect(self.addr)
            .await
            .expect("connect to the server");
        let request = format!(
            "{method} {target} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nConnection: close\r\n\r\n",
            self.addr.port()
        );
        stream
            .write_all(request.as_bytes())
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
        self.request("GET", target).await
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
            .expect("the status line should carry a numeric status");
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
}

fn write(dir: &TempDir, relative: &str, bytes: &[u8]) {
    let path = dir.path().join(relative);
    fs::create_dir_all(path.parent().expect("the parent directory")).expect("create parent");
    fs::write(path, bytes).expect("write the file");
}

fn write_fixture(dir: &TempDir) {
    write(dir, "sdlc/PRD.md", b"# Product requirements\n");
    write(dir, "sdlc/architecture.md", b"# Architecture\n");
    write(dir, "sdlc/conventions.md", b"# Conventions\n");
    write(dir, "AGENTS.md", b"# Agent rules\n");
    write(
        dir,
        "sdlc/epics/0001-foundation/description.md",
        b"---\nstatus: planned\n---\n\n# 0001: Foundation\n",
    );
    write(
        dir,
        "sdlc/epics/0001-foundation/tickets/0001-project.md",
        b"---\nstatus: todo\nfeature: export\n---\n\n# 0001: Project skeleton\n",
    );
    write(
        dir,
        "sdlc/decisions/0003-use-axum.md",
        b"---\nstatus: proposed\n---\n\n# 0003: Use axum\n",
    );
    write(
        dir,
        "sdlc/features/export.md",
        b"---\nstatus: shipped\n---\n\n# Export\n",
    );
}

#[tokio::test]
async fn tree_returns_the_store_shape_exactly() {
    let dir = TempDir::new().expect("temp project");
    write_fixture(&dir);
    let server = TestServer::start(dir.path()).await;

    let response = server.get("/api/tree").await;

    assert_eq!(response.status, 200);
    assert_eq!(response.header("content-type"), Some("application/json"));
    assert_eq!(response.header("cache-control"), Some("no-store"));
    assert_eq!(
        response.json(),
        serde_json::json!({
            "layout_exists": true,
            "docs": [
                {
                    "path": "sdlc/PRD.md",
                    "kind": "core",
                    "title": "Product requirements",
                    "status": null,
                    "supersedes": null,
                    "epic": null,
                    "feature": null,
                    "depends_on": [],
                    "orphaned": false,
                    "feature_orphaned": false
                },
                {
                    "path": "sdlc/architecture.md",
                    "kind": "core",
                    "title": "Architecture",
                    "status": null,
                    "supersedes": null,
                    "epic": null,
                    "feature": null,
                    "depends_on": [],
                    "orphaned": false,
                    "feature_orphaned": false
                },
                {
                    "path": "sdlc/conventions.md",
                    "kind": "core",
                    "title": "Conventions",
                    "status": null,
                    "supersedes": null,
                    "epic": null,
                    "feature": null,
                    "depends_on": [],
                    "orphaned": false,
                    "feature_orphaned": false
                },
                {
                    "path": "AGENTS.md",
                    "kind": "core",
                    "title": "Agent rules",
                    "status": null,
                    "supersedes": null,
                    "epic": null,
                    "feature": null,
                    "depends_on": [],
                    "orphaned": false,
                    "feature_orphaned": false
                },
                {
                    "path": "sdlc/epics/0001-foundation/description.md",
                    "kind": "epic",
                    "title": "0001: Foundation",
                    "status": "planned",
                    "supersedes": null,
                    "epic": null,
                    "feature": null,
                    "depends_on": [],
                    "orphaned": false,
                    "feature_orphaned": false
                },
                {
                    "path": "sdlc/epics/0001-foundation/tickets/0001-project.md",
                    "kind": "ticket",
                    "title": "0001: Project skeleton",
                    "status": "todo",
                    "supersedes": null,
                    "epic": "0001-foundation",
                    "feature": "export",
                    "depends_on": [],
                    "orphaned": false,
                    "feature_orphaned": false
                },
                {
                    "path": "sdlc/decisions/0003-use-axum.md",
                    "kind": "decision",
                    "title": "0003: Use axum",
                    "status": "proposed",
                    "supersedes": null,
                    "epic": null,
                    "feature": null,
                    "depends_on": [],
                    "orphaned": false,
                    "feature_orphaned": false
                },
                {
                    "path": "sdlc/features/export.md",
                    "kind": "feature",
                    "title": "Export",
                    "status": "shipped",
                    "supersedes": null,
                    "epic": null,
                    "feature": null,
                    "depends_on": [],
                    "orphaned": false,
                    "feature_orphaned": false
                }
            ]
        })
    );
}

#[tokio::test]
async fn tree_reports_a_missing_layout() {
    let dir = TempDir::new().expect("temp project");
    let server = TestServer::start(dir.path()).await;

    let response = server.get("/api/tree").await;

    assert_eq!(response.status, 200);
    assert_eq!(
        response.json(),
        serde_json::json!({ "layout_exists": false, "docs": [] })
    );
}

#[tokio::test]
async fn tree_exposes_the_ticket_orphan_and_dependency_fields() {
    let dir = TempDir::new().expect("temp project");
    write(
        &dir,
        "sdlc/epics/0002-ghost/tickets/0001-orphan.md",
        b"---\nstatus: todo\nfeature: export\ndepends_on: [\"0004\", \"0005\"]\n---\n\n# 0001: Orphan ticket\n",
    );
    let server = TestServer::start(dir.path()).await;

    let response = server.get("/api/tree").await;

    assert_eq!(response.status, 200);
    let tree_json = response.json();
    let docs = tree_json["docs"]
        .as_array()
        .expect("the tree should carry docs");
    assert_eq!(docs.len(), 1);
    let entry = &docs[0];
    assert_eq!(entry["kind"], "ticket");
    assert_eq!(entry["epic"], "0002-ghost");
    assert_eq!(entry["orphaned"], true);
    assert_eq!(entry["feature_orphaned"], true);
    assert_eq!(entry["depends_on"], serde_json::json!([4, 5]));
}

#[tokio::test]
async fn read_returns_exact_content_and_the_current_hash() {
    let dir = TempDir::new().expect("temp project");
    let bytes = b"# PRD\r\n\r\nBody with a check mark \xe2\x9c\x93\n";
    write(&dir, "sdlc/PRD.md", bytes);
    let server = TestServer::start(dir.path()).await;

    let response = server.get("/api/docs/sdlc/PRD.md").await;

    assert_eq!(response.status, 200);
    assert_eq!(response.header("content-type"), Some("application/json"));
    assert_eq!(response.header("cache-control"), Some("no-store"));
    let json = response.json();
    assert_eq!(json["path"].as_str(), Some("sdlc/PRD.md"));
    assert_eq!(
        json["content"].as_str(),
        Some(std::str::from_utf8(bytes).expect("the fixture is UTF-8"))
    );
    assert_eq!(json["hash"].as_str(), Some(content_hash(bytes).as_str()));
}

#[tokio::test]
async fn reading_again_after_an_external_write_returns_the_new_hash() {
    let dir = TempDir::new().expect("temp project");
    write(&dir, "sdlc/PRD.md", b"# First\n");
    let server = TestServer::start(dir.path()).await;

    let first = server.get("/api/docs/sdlc/PRD.md").await.json();
    let first_hash = first["hash"].as_str().expect("the first hash").to_string();

    write(&dir, "sdlc/PRD.md", b"# Second\n");
    let second = server.get("/api/docs/sdlc/PRD.md").await.json();

    assert_eq!(second["content"].as_str(), Some("# Second\n"));
    let second_hash = second["hash"].as_str().expect("the second hash");
    assert_ne!(second_hash, first_hash);
    assert_eq!(second_hash, content_hash(b"# Second\n"));
}

#[tokio::test]
async fn a_missing_document_returns_json_404() {
    let dir = TempDir::new().expect("temp project");
    write_fixture(&dir);
    let server = TestServer::start(dir.path()).await;

    let response = server.get("/api/docs/sdlc/features/missing.md").await;

    assert_eq!(response.status, 404);
    assert_eq!(response.header("content-type"), Some("application/json"));
    let json = response.json();
    assert_eq!(json["error"]["code"].as_str(), Some("not_found"));
    assert!(
        !response.body.contains("missing.md"),
        "the path is not exposed"
    );
}

#[tokio::test]
async fn traversal_absolute_paths_and_unknown_layout_are_rejected() {
    let dir = TempDir::new().expect("temp project");
    write_fixture(&dir);
    let server = TestServer::start(dir.path()).await;

    for target in [
        "/api/docs/../Cargo.toml",
        "/api/docs/%2e%2e%2fCargo.toml",
        "/api/docs/%2Fetc%2Fpasswd",
        "/api/docs/sdlc/unknown/PRD.md",
    ] {
        let response = server.get(target).await;

        assert_eq!(response.status, 400, "{target}");
        assert_eq!(response.header("content-type"), Some("application/json"));
        let json = response.json();
        assert_eq!(
            json["error"]["code"].as_str(),
            Some("not_allowed"),
            "{target}"
        );
        assert!(!response.body.contains("[dependencies]"));
        assert!(!response.body.contains("/etc/passwd"));
    }
}

#[tokio::test]
async fn invalid_utf8_returns_a_stable_error_without_a_panic() {
    let dir = TempDir::new().expect("temp project");
    write(&dir, "sdlc/features/broken.md", b"# Broken\n\xff\xfe");
    let server = TestServer::start(dir.path()).await;

    let first = server.get("/api/docs/sdlc/features/broken.md").await;
    let second = server.get("/api/docs/sdlc/features/broken.md").await;

    assert!(
        first.status == 400 || first.status == 500,
        "unexpected status {}",
        first.status
    );
    assert_eq!(first.header("content-type"), Some("application/json"));
    let first_json = first.json();
    let second_json = second.json();
    assert_eq!(first_json["error"]["code"], second_json["error"]["code"]);
    assert_eq!(
        first_json["error"]["code"].as_str(),
        Some("invalid_utf8"),
        "the code should be stable"
    );
    assert!(!first.body.contains("broken.md"), "the path is not exposed");

    let tree = server.get("/api/tree").await;
    assert_eq!(tree.status, 200, "the server should keep serving requests");
}

#[tokio::test]
async fn head_reads_without_saving_and_returns_no_body() {
    let dir = TempDir::new().expect("temp project");
    write(&dir, "sdlc/PRD.md", b"# Keep me\n");
    let server = TestServer::start(dir.path()).await;

    let response = server.request("HEAD", "/api/docs/sdlc/PRD.md").await;

    assert_eq!(response.status, 200);
    assert!(response.body.is_empty());
    assert_eq!(response.header("cache-control"), Some("no-store"));
    assert_eq!(
        fs::read(dir.path().join("sdlc/PRD.md")).expect("read the file"),
        b"# Keep me\n"
    );
}

#[tokio::test]
async fn unknown_api_routes_return_json_not_the_spa_page() {
    let dir = TempDir::new().expect("temp project");
    let server = TestServer::start(dir.path()).await;

    let response = server.get("/api/not-a-route").await;

    assert_eq!(response.status, 404);
    assert_eq!(response.header("content-type"), Some("application/json"));
    assert_eq!(response.json()["error"]["code"].as_str(), Some("not_found"));
}
