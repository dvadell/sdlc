//! Integration tests for the PRD feature index refresh (ticket 0029).

use std::fs;
use std::net::SocketAddr;
use std::path::Path;

use sdlc::config::{CliArgs, Config};
use sdlc::store::content_hash;
use sdlc::store::{DocKind, DocPath, DocStore};
use sdlc::{ServerError, serve};
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;

/// The starter PRD of a scaffolded project, with the marked region still empty.
const PRD_WITH_MARKERS: &str = "# Product Requirements Document\n\n## Purpose\n\nTODO: write the purpose of this document in one or two short paragraphs.\n\n<!-- sdlc:feature-index:start -->\n<!-- sdlc:feature-index:end -->\n";

/// A feature document with a planned status.
const FEATURE_WITH_STATUS: &str = "---\nstatus: planned\n---\n\n# Export\n";

/// The expected marked region after one planned feature exists.
const PRD_WITH_FEATURE: &str = "# Product Requirements Document\n\n## Purpose\n\nTODO: write the purpose of this document in one or two short paragraphs.\n\n<!-- sdlc:feature-index:start -->\n- [Export](features/export.md) — planned\n<!-- sdlc:feature-index:end -->\n";

/// The expected marked region after the feature shipped.
const PRD_WITH_FEATURE_SHIPPED: &str = "# Product Requirements Document\n\n## Purpose\n\nTODO: write the purpose of this document in one or two short paragraphs.\n\n<!-- sdlc:feature-index:start -->\n- [Export](features/export.md) — shipped\n<!-- sdlc:feature-index:end -->\n";

/// The expected marked region once no feature is left.
const PRD_EMPTY_REGION: &str = "# Product Requirements Document\n\n## Purpose\n\nTODO: write the purpose of this document in one or two short paragraphs.\n\n<!-- sdlc:feature-index:start -->\n<!-- sdlc:feature-index:end -->\n";

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

    async fn post(&self, target: &str, body: &str) -> HttpResponse {
        self.request("POST", target, Some("application/json"), body)
            .await
    }

    async fn put(&self, target: &str, body: &str) -> HttpResponse {
        self.request("PUT", target, Some("application/json"), body)
            .await
    }

    async fn create(&self, body: Value) -> HttpResponse {
        self.post("/api/docs", &body.to_string()).await
    }

    async fn move_doc(&self, from: &str, to: &str) -> HttpResponse {
        self.post(
            "/api/docs/move",
            &json!({ "from": from, "to": to }).to_string(),
        )
        .await
    }

    async fn delete(&self, target: &str) -> HttpResponse {
        self.request("DELETE", target, Some("application/json"), "")
            .await
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

/// The status and body of one raw HTTP response.
struct HttpResponse {
    status: u16,
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
        let (_, body) = response
            .split_once("\r\n\r\n")
            .expect("the response should have a head and a body");

        Self {
            status,
            body: body.to_string(),
        }
    }

    fn json(&self) -> Value {
        serde_json::from_str(&self.body).expect("the response body should be JSON")
    }

    fn path(&self) -> String {
        self.json()["path"]
            .as_str()
            .expect("the response should carry a path")
            .to_string()
    }
}

fn write(dir: &TempDir, relative: &str, content: &str) {
    let path = dir.path().join(relative);
    fs::create_dir_all(path.parent().expect("the parent directory")).expect("create parent");
    fs::write(path, content).expect("write the file");
}

/// Starts a scaffolded project and returns its directory and server.
async fn scaffolded_server() -> (TempDir, TestServer) {
    let dir = TempDir::new().expect("temp project");
    let server = TestServer::start(dir.path()).await;
    let scaffolded = server.post("/api/scaffold", "{}").await;
    assert_eq!(scaffolded.status, 200);

    (dir, server)
}

/// Creates a planned feature through the API and returns the create response.
async fn create_planned_feature(server: &TestServer) -> HttpResponse {
    server
        .create(json!({ "kind": "feature", "title": "Export" }))
        .await
}

#[tokio::test]
async fn create_feature_adds_a_line_to_the_prd_feature_index() {
    let (dir, server) = scaffolded_server().await;
    let feature = create_planned_feature(&server).await;

    assert_eq!(feature.status, 201);
    let prd = server.get("/api/docs/sdlc/PRD.md").await;
    assert_eq!(prd.status, 200);
    assert_eq!(
        prd.json()["content"].as_str().expect("the PRD content"),
        PRD_WITH_FEATURE
    );
    assert!(dir.path().join("sdlc/features/export.md").is_file());
}

#[tokio::test]
async fn create_feature_updates_the_index_without_warnings() {
    let (_dir, server) = scaffolded_server().await;
    let feature = create_planned_feature(&server).await;

    assert_eq!(feature.status, 201);
    assert!(
        feature.json().get("warnings").is_none(),
        "an updated index carries no warning: {}",
        feature.json()
    );
}

#[tokio::test]
async fn create_of_a_non_feature_document_has_no_index_warnings() {
    let (_dir, server) = scaffolded_server().await;
    let response = server
        .create(json!({ "kind": "epic", "title": "Foundation" }))
        .await;

    assert_eq!(response.status, 201);
    assert!(
        response.json().get("warnings").is_none(),
        "a non-feature create must not refresh the PRD index: {}",
        response.json()
    );
}

#[tokio::test]
async fn create_feature_without_markers_returns_201_with_a_warning() {
    let dir = TempDir::new().expect("temp project");
    write(
        &dir,
        "sdlc/PRD.md",
        "# Product Requirements Document\n\nNo index yet.\n",
    );
    let server = TestServer::start(dir.path()).await;

    let feature = create_planned_feature(&server).await;

    assert_eq!(feature.status, 201);
    let feature_json = feature.json();
    let warnings = feature_json["warnings"].as_array().expect("the warnings");
    assert_eq!(warnings.len(), 1);
    assert!(dir.path().join("sdlc/features/export.md").is_file());
}

#[tokio::test]
async fn rename_feature_updates_the_prd_feature_index_line() {
    let (_dir, server) = scaffolded_server().await;
    let feature = create_planned_feature(&server).await;
    assert_eq!(feature.status, 201);

    let moved = server
        .move_doc("sdlc/features/export.md", "sdlc/features/csv-export.md")
        .await;
    assert_eq!(moved.status, 200);
    assert_eq!(moved.path(), "sdlc/features/csv-export.md");

    let prd = server.get("/api/docs/sdlc/PRD.md").await;
    assert_eq!(prd.status, 200);
    assert!(
        prd.json()["content"]
            .as_str()
            .expect("the PRD content")
            .contains("- [Export](features/csv-export.md) — planned")
    );
    assert!(
        !prd.json()["content"]
            .as_str()
            .expect("the PRD content")
            .contains("features/export.md")
    );
}

#[tokio::test]
async fn delete_feature_removes_the_prd_feature_index_line() {
    let (dir, server) = scaffolded_server().await;
    let feature = create_planned_feature(&server).await;
    assert_eq!(feature.status, 201);

    let deleted = server.delete("/api/docs/sdlc/features/export.md").await;
    assert_eq!(deleted.status, 200);

    let prd = server.get("/api/docs/sdlc/PRD.md").await;
    assert_eq!(prd.status, 200);
    assert_eq!(
        prd.json()["content"].as_str().expect("the PRD content"),
        PRD_EMPTY_REGION
    );
    assert!(!dir.path().join("sdlc/features/export.md").exists());
}

#[tokio::test]
async fn saving_a_feature_status_updates_the_prd_feature_index_line() {
    let dir = TempDir::new().expect("temp project");
    write(&dir, "sdlc/PRD.md", PRD_WITH_MARKERS);
    write(&dir, "sdlc/features/export.md", FEATURE_WITH_STATUS);
    let server = TestServer::start(dir.path()).await;

    let read = server.get("/api/docs/sdlc/features/export.md").await.json();
    let old_hash = read["hash"].as_str().expect("the feature hash").to_string();
    let response = server
        .put(
            "/api/docs/sdlc/features/export.md",
            &json!({
                "content": "---\nstatus: shipped\n---\n\n# Export\n",
                "expected_hash": old_hash,
            })
            .to_string(),
        )
        .await;
    assert_eq!(response.status, 200);

    let prd = server.get("/api/docs/sdlc/PRD.md").await;
    assert_eq!(prd.status, 200);
    assert_eq!(
        prd.json()["content"].as_str().expect("the PRD content"),
        PRD_WITH_FEATURE_SHIPPED
    );
}

#[tokio::test]
async fn saving_a_feature_twice_without_changes_leaves_the_prd_untouched() {
    let dir = TempDir::new().expect("temp project");
    write(&dir, "sdlc/PRD.md", PRD_WITH_MARKERS);
    write(&dir, "sdlc/features/export.md", FEATURE_WITH_STATUS);
    let server = TestServer::start(dir.path()).await;

    let first = server
        .put(
            "/api/docs/sdlc/features/export.md",
            &json!({
                "content": FEATURE_WITH_STATUS,
                "expected_hash": content_hash(FEATURE_WITH_STATUS.as_bytes()),
            })
            .to_string(),
        )
        .await;
    assert_eq!(first.status, 200);

    let prd_path = dir.path().join("sdlc/PRD.md");
    let prd_before = fs::read_to_string(&prd_path).expect("read the PRD");
    let mtime_before = fs::metadata(&prd_path)
        .expect("read the PRD metadata")
        .modified()
        .expect("read the PRD mtime");

    let second = server
        .put(
            "/api/docs/sdlc/features/export.md",
            &json!({
                "content": FEATURE_WITH_STATUS,
                "expected_hash": content_hash(FEATURE_WITH_STATUS.as_bytes()),
            })
            .to_string(),
        )
        .await;
    assert_eq!(second.status, 200);

    let prd_after = fs::read_to_string(&prd_path).expect("read the PRD");
    assert_eq!(prd_before, prd_after);
    assert_eq!(
        fs::metadata(&prd_path)
            .expect("read the PRD metadata")
            .modified()
            .expect("read the PRD mtime"),
        mtime_before
    );
}

#[tokio::test]
async fn store_create_feature_refreshes_the_prd() {
    let dir = TempDir::new().expect("temp project");
    let store = DocStore::new(dir.path().to_path_buf()).expect("the store should build");
    let _ = store.scaffold().await.expect("the scaffold should run");
    let _path = store
        .create(DocKind::Feature, "Export", None, None)
        .await
        .expect("the feature should be created");
    let warnings = store.refresh_feature_index().await;
    assert!(warnings.is_empty());

    let prd = store
        .read(&DocPath::parse("sdlc/PRD.md").expect("the prd path"))
        .await
        .expect("the prd should be readable");
    assert_eq!(prd.content, PRD_WITH_FEATURE);
}

#[tokio::test]
async fn saving_a_non_feature_document_leaves_the_prd_untouched() {
    let (dir, server) = scaffolded_server().await;
    let feature = create_planned_feature(&server).await;
    assert_eq!(feature.status, 201);

    let architecture = server.get("/api/docs/sdlc/architecture.md").await.json();
    let architecture_hash = architecture["hash"]
        .as_str()
        .expect("the architecture hash")
        .to_string();
    let response = server
        .put(
            "/api/docs/sdlc/architecture.md",
            &json!({
                "content": "# Architecture\n\nA new section.\n",
                "expected_hash": architecture_hash,
            })
            .to_string(),
        )
        .await;
    assert_eq!(response.status, 200);

    let prd = server.get("/api/docs/sdlc/PRD.md").await;
    assert_eq!(prd.status, 200);
    assert!(
        prd.json()["content"]
            .as_str()
            .expect("the PRD content")
            .contains("- [Export](features/export.md) — planned")
    );
    assert!(dir.path().join("sdlc/architecture.md").is_file());
}
