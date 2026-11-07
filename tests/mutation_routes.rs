//! Integration tests for create, move, delete and scaffold routes (ticket 0020).

use std::fs;
use std::net::SocketAddr;
use std::path::Path;

use sdlc::config::{CliArgs, Config};
use sdlc::store::content_hash;
use sdlc::{ServerError, serve};
use serde_json::{Value, json};
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

    async fn get(&self, target: &str) -> HttpResponse {
        self.request("GET", target, None, "").await
    }

    async fn post(&self, target: &str, body: &str) -> HttpResponse {
        self.request("POST", target, Some("application/json"), body)
            .await
    }

    async fn create(&self, body: Value) -> HttpResponse {
        self.post("/api/docs", &body.to_string()).await
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

fn assert_error(response: &HttpResponse, status: u16, code: &str) {
    assert_eq!(response.status, status, "body: {}", response.body);
    assert_eq!(response.header("content-type"), Some("application/json"));
    assert_eq!(response.error_code().as_deref(), Some(code));
}

fn tree_paths(tree: &Value) -> Vec<String> {
    tree["docs"]
        .as_array()
        .expect("the tree should carry docs")
        .iter()
        .map(|doc| {
            doc["path"]
                .as_str()
                .expect("each tree entry should carry a path")
                .to_string()
        })
        .collect()
}

#[tokio::test]
async fn create_each_kind_returns_201_and_the_document_appears_in_the_tree() {
    let dir = TempDir::new().expect("temp project");
    let server = TestServer::start(dir.path()).await;

    let epic = server
        .create(json!({ "kind": "epic", "title": "Foundation" }))
        .await;
    assert_eq!(epic.status, 201);
    assert_eq!(epic.header("content-type"), Some("application/json"));
    let epic_path = epic.path();
    assert_eq!(epic_path, "sdlc/epics/0001-foundation/description.md");
    let epic_file = dir.path().join(&epic_path);
    let epic_content = fs::read_to_string(&epic_file).expect("read the created epic");
    assert!(epic_content.starts_with("---\nstatus: planned\n---\n\n"));
    assert!(epic_content.contains("# 0001: Foundation"));
    assert_eq!(
        epic.json()["hash"].as_str(),
        Some(content_hash(epic_content.as_bytes()).as_str())
    );
    assert!(
        dir.path()
            .join("sdlc/epics/0001-foundation/tickets")
            .is_dir()
    );

    let feature = server
        .create(json!({ "kind": "feature", "title": "Export" }))
        .await;
    assert_eq!(feature.status, 201);
    assert_eq!(feature.path(), "sdlc/features/export.md");

    let decision = server
        .create(json!({ "kind": "decision", "title": "Use axum" }))
        .await;
    assert_eq!(decision.status, 201);
    assert_eq!(decision.path(), "sdlc/decisions/0001-use-axum.md");

    let ticket = server
        .create(json!({
            "kind": "ticket",
            "title": "Add CSV export",
            "epic": "0001-foundation",
            "feature": "export"
        }))
        .await;
    assert_eq!(ticket.status, 201);
    let ticket_path = ticket.path();
    assert_eq!(
        ticket_path,
        "sdlc/epics/0001-foundation/tickets/0001-add-csv-export.md"
    );
    assert_eq!(ticket.json()["hash"].as_str().map(str::len), Some(64));

    let tree = server.get("/api/tree").await;
    assert_eq!(tree.status, 200);
    let paths = tree_paths(&tree.json());
    for path in [
        &epic_path,
        &"sdlc/features/export.md".to_string(),
        &"sdlc/decisions/0001-use-axum.md".to_string(),
        &ticket_path,
    ] {
        assert!(paths.contains(path), "missing {path} from {paths:?}");
    }

    let tree_json = tree.json();
    let ticket_entry = tree_json["docs"]
        .as_array()
        .expect("docs")
        .iter()
        .find(|doc| doc["path"] == ticket_path)
        .expect("the created ticket should be in the tree");
    assert_eq!(ticket_entry["kind"], "ticket");
    assert_eq!(ticket_entry["epic"], "0001-foundation");
    assert_eq!(ticket_entry["feature"], "export");
    assert_eq!(ticket_entry["status"], "todo");
}

#[tokio::test]
async fn create_decision_defaults_to_a_proposed_status_and_accepts_each_decision_status() {
    let dir = TempDir::new().expect("temp project");
    let server = TestServer::start(dir.path()).await;

    let proposed = server
        .create(json!({ "kind": "decision", "title": "Use axum" }))
        .await;
    assert_eq!(proposed.status, 201);
    assert_eq!(proposed.path(), "sdlc/decisions/0001-use-axum.md");
    let proposed_content = fs::read_to_string(dir.path().join("sdlc/decisions/0001-use-axum.md"))
        .expect("read the created decision");
    assert!(proposed_content.starts_with("---\nstatus: proposed\n---\n\n"));

    let accepted = server
        .create(json!({ "kind": "decision", "title": "Use Postgres", "status": "accepted" }))
        .await;
    assert_eq!(accepted.path(), "sdlc/decisions/0002-use-postgres.md");
    let accepted_content =
        fs::read_to_string(dir.path().join("sdlc/decisions/0002-use-postgres.md"))
            .expect("read the created decision");
    assert!(accepted_content.starts_with("---\nstatus: accepted\n---\n\n"));

    let superseded = server
        .create(json!({ "kind": "decision", "title": "Use SQLite", "status": "superseded" }))
        .await;
    assert_eq!(superseded.path(), "sdlc/decisions/0003-use-sqlite.md");
    let superseded_content =
        fs::read_to_string(dir.path().join("sdlc/decisions/0003-use-sqlite.md"))
            .expect("read the created decision");
    assert!(superseded_content.starts_with("---\nstatus: superseded\n---\n\n"));
}

#[tokio::test]
async fn create_decision_with_an_invalid_status_returns_400_and_writes_nothing() {
    let dir = TempDir::new().expect("temp project");
    let server = TestServer::start(dir.path()).await;

    let response = server
        .create(json!({ "kind": "decision", "title": "Use axum", "status": "bogus" }))
        .await;

    assert_error(&response, 400, "bad_request");
    assert!(!dir.path().join("sdlc/decisions").exists());
}

#[tokio::test]
async fn create_decision_with_a_non_numeric_supersedes_target_returns_400_and_writes_nothing() {
    let dir = TempDir::new().expect("temp project");
    let server = TestServer::start(dir.path()).await;

    let response = server
        .create(json!({ "kind": "decision", "title": "Use axum", "supersedes": "not-a-number" }))
        .await;

    assert_error(&response, 400, "bad_request");
    assert!(!dir.path().join("sdlc/decisions").exists());
}

#[tokio::test]
async fn create_decision_that_supersedes_itself_returns_400_and_writes_nothing() {
    let dir = TempDir::new().expect("temp project");
    let server = TestServer::start(dir.path()).await;

    // The first decision of a project is numbered `0001`, so a pointer to it
    // names the decision being created.
    let response = server
        .create(json!({ "kind": "decision", "title": "Use axum", "supersedes": 1 }))
        .await;

    assert_error(&response, 400, "bad_request");
    assert!(!dir.path().join("sdlc/decisions").exists());
}

#[tokio::test]
async fn create_decision_with_a_missing_supersedes_target_returns_201_with_a_warning() {
    let dir = TempDir::new().expect("temp project");
    let server = TestServer::start(dir.path()).await;

    let response = server
        .create(json!({ "kind": "decision", "title": "Use axum", "supersedes": 42 }))
        .await;

    assert_eq!(response.status, 201);
    let path = response.path();
    assert_eq!(path, "sdlc/decisions/0001-use-axum.md");
    let content = fs::read_to_string(dir.path().join(&path)).expect("read the created decision");
    assert!(content.starts_with("---\nstatus: proposed\nsupersedes: 42\n---\n\n"));

    let json = response.json();
    let warnings = json["warnings"]
        .as_array()
        .expect("a missing supersedes target is reported as a create warning");
    assert_eq!(warnings.len(), 1, "the body was: {}", response.body);
    assert!(
        warnings[0]
            .as_str()
            .expect("the warning is a string")
            .contains("42")
    );

    let tree = server.get("/api/tree").await.json();
    let entry = tree["docs"]
        .as_array()
        .expect("docs")
        .iter()
        .find(|doc| doc["path"] == path)
        .expect("the created decision should be in the tree");
    assert_eq!(entry["kind"], "decision");
    assert_eq!(entry["status"], "proposed");
    assert_eq!(entry["supersedes"], json!(42));
}

#[tokio::test]
async fn create_decision_with_a_blank_status_defaults_to_proposed() {
    let dir = TempDir::new().expect("temp project");
    let server = TestServer::start(dir.path()).await;

    let response = server
        .create(json!({ "kind": "decision", "title": "Use axum", "status": "" }))
        .await;

    assert_eq!(response.status, 201);
    let path = response.path();
    assert_eq!(path, "sdlc/decisions/0001-use-axum.md");
    let content = fs::read_to_string(dir.path().join(&path)).expect("read the created decision");
    assert!(content.starts_with("---\nstatus: proposed\n---\n\n"));
}

#[tokio::test]
async fn create_decision_with_a_non_positive_supersedes_target_returns_400_and_writes_nothing() {
    let dir = TempDir::new().expect("temp project");
    let server = TestServer::start(dir.path()).await;

    let response = server
        .create(json!({ "kind": "decision", "title": "Use axum", "supersedes": 0 }))
        .await;

    assert_error(&response, 400, "bad_request");
    assert!(!dir.path().join("sdlc/decisions").exists());
}

#[tokio::test]
async fn create_decision_superseding_an_existing_decision_has_no_create_warning() {
    let dir = TempDir::new().expect("temp project");
    let server = TestServer::start(dir.path()).await;

    let first = server
        .create(json!({ "kind": "decision", "title": "Use axum" }))
        .await;
    assert_eq!(first.status, 201);

    let second = server
        .create(json!({
            "kind": "decision",
            "title": "Use Postgres",
            "status": "accepted",
            "supersedes": 1
        }))
        .await;

    assert_eq!(second.status, 201);
    let json = second.json();
    assert!(
        json.get("warnings").is_none(),
        "an existing supersedes target carries no warning; the body was: {}",
        second.body
    );
}

#[tokio::test]
async fn create_decision_with_a_status_for_a_feature_returns_400_and_writes_nothing() {
    let dir = TempDir::new().expect("temp project");
    let server = TestServer::start(dir.path()).await;

    let response = server
        .create(json!({ "kind": "feature", "title": "Export", "status": "proposed" }))
        .await;

    assert_error(&response, 400, "bad_request");
    assert!(!dir.path().join("sdlc/features").exists());
}

#[tokio::test]
async fn create_a_decision_with_an_epic_reference_returns_400_and_writes_nothing() {
    let dir = TempDir::new().expect("temp project");
    let server = TestServer::start(dir.path()).await;

    let response = server
        .create(json!({ "kind": "decision", "title": "Use axum", "epic": "0001-foundation" }))
        .await;

    assert_error(&response, 400, "bad_request");
    assert!(!dir.path().join("sdlc/decisions").exists());
}

#[tokio::test]
async fn create_epic_uses_the_next_number_and_keeps_an_existing_epic() {
    let dir = TempDir::new().expect("temp project");
    write(
        &dir,
        "sdlc/epics/0003-platform/description.md",
        "---\nstatus: planned\n---\n\n# 0003: Platform\n",
    );
    let server = TestServer::start(dir.path()).await;

    let response = server
        .create(json!({ "kind": "epic", "title": "Document API" }))
        .await;

    assert_eq!(response.status, 201);
    assert_eq!(
        response.path(),
        "sdlc/epics/0004-document-api/description.md"
    );
    let content = fs::read_to_string(
        dir.path()
            .join("sdlc/epics/0004-document-api/description.md"),
    )
    .expect("read the created epic");
    assert!(content.starts_with("---\nstatus: planned\n---\n\n"));
    assert!(content.contains("# 0004: Document API"));
    assert!(
        dir.path()
            .join("sdlc/epics/0003-platform/description.md")
            .is_file()
    );
}

#[tokio::test]
async fn create_ticket_without_an_epic_returns_400() {
    let dir = TempDir::new().expect("temp project");
    let server = TestServer::start(dir.path()).await;

    let response = server
        .create(json!({ "kind": "ticket", "title": "Add export" }))
        .await;

    assert_error(&response, 400, "bad_request");
    assert!(!dir.path().join("sdlc/epics").exists());
}

#[tokio::test]
async fn create_ticket_with_an_unknown_epic_returns_404() {
    let dir = TempDir::new().expect("temp project");
    let server = TestServer::start(dir.path()).await;

    let response = server
        .create(json!({
            "kind": "ticket",
            "title": "Add export",
            "epic": "9999-missing"
        }))
        .await;

    assert_error(&response, 404, "not_found");
    assert!(!dir.path().join("sdlc").exists());
}

#[tokio::test]
async fn create_ticket_with_an_unknown_feature_returns_400() {
    let dir = TempDir::new().expect("temp project");
    let server = TestServer::start(dir.path()).await;
    server
        .create(json!({ "kind": "epic", "title": "Foundation" }))
        .await;

    let response = server
        .create(json!({
            "kind": "ticket",
            "title": "Add export",
            "epic": "0001-foundation",
            "feature": "missing"
        }))
        .await;

    assert_error(&response, 400, "bad_request");
    let tickets = fs::read_dir(dir.path().join("sdlc/epics/0001-foundation/tickets"))
        .expect("read the tickets directory")
        .flatten()
        .count();
    assert_eq!(tickets, 0, "no ticket should be created");
}

#[tokio::test]
async fn create_ticket_without_a_feature_succeeds_and_derives_its_epic_from_the_path() {
    let dir = TempDir::new().expect("temp project");
    let server = TestServer::start(dir.path()).await;
    server
        .create(json!({ "kind": "epic", "title": "Foundation" }))
        .await;

    let response = server
        .create(json!({
            "kind": "ticket",
            "title": "Add export",
            "epic": "0001-foundation"
        }))
        .await;

    assert_eq!(response.status, 201);
    let path = response.path();
    assert_eq!(
        path,
        "sdlc/epics/0001-foundation/tickets/0001-add-export.md"
    );
    let content = fs::read_to_string(dir.path().join(&path)).expect("read the ticket");
    assert!(!content.contains("feature:"));
    assert!(!content.contains("epic:"));
    assert_eq!(content.matches("# 0001: Add export").count(), 1);

    let tree = server.get("/api/tree").await.json();
    let entry = tree["docs"]
        .as_array()
        .expect("docs")
        .iter()
        .find(|doc| doc["path"] == path)
        .expect("the ticket should be listed");
    assert_eq!(entry["epic"], "0001-foundation");
    assert_eq!(entry["feature"], Value::Null);
}

#[tokio::test]
async fn create_with_an_invalid_kind_returns_400() {
    let dir = TempDir::new().expect("temp project");
    let server = TestServer::start(dir.path()).await;

    let response = server
        .create(json!({ "kind": "prd", "title": "Nope" }))
        .await;

    assert_error(&response, 400, "bad_request");
}

#[tokio::test]
async fn create_duplicate_feature_returns_409_and_keeps_the_original() {
    let dir = TempDir::new().expect("temp project");
    let server = TestServer::start(dir.path()).await;
    let first = server
        .create(json!({ "kind": "feature", "title": "Export" }))
        .await;
    assert_eq!(first.status, 201);
    let original = fs::read_to_string(dir.path().join("sdlc/features/export.md"))
        .expect("read the original feature");

    let duplicate = server
        .create(json!({ "kind": "feature", "title": "Export" }))
        .await;

    assert_error(&duplicate, 409, "conflict");
    assert_eq!(
        fs::read_to_string(dir.path().join("sdlc/features/export.md"))
            .expect("read the feature again"),
        original
    );
}

#[tokio::test]
async fn create_titles_with_path_metacharacters_cannot_escape_the_layout() {
    let dir = TempDir::new().expect("temp project");
    let server = TestServer::start(dir.path()).await;

    let response = server
        .create(json!({ "kind": "feature", "title": "../escape/../../pwn" }))
        .await;
    assert_eq!(response.status, 201);
    let path = response.path();
    assert!(path.starts_with("sdlc/features/"), "path: {path}");
    assert!(!path.contains(".."), "path: {path}");
    assert!(dir.path().join(&path).is_file());
    assert!(
        !dir.path()
            .parent()
            .expect("temp parent")
            .join("pwn.md")
            .exists()
    );

    for title in ["..", "line\nbreak"] {
        let response = server
            .create(json!({ "kind": "feature", "title": title }))
            .await;
        assert_error(&response, 400, "bad_request");
    }

    let names: Vec<String> = fs::read_dir(dir.path().join("sdlc/features"))
        .expect("read the features directory")
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, vec!["escapepwn.md".to_string()]);
}

#[tokio::test]
async fn move_returns_the_new_path_and_keeps_the_content() {
    let dir = TempDir::new().expect("temp project");
    write(
        &dir,
        "sdlc/features/export.md",
        "---\nstatus: planned\n---\n\n# Export\n",
    );
    let server = TestServer::start(dir.path()).await;

    let response = server
        .post(
            "/api/docs/move",
            &json!({
                "from": "sdlc/features/export.md",
                "to": "sdlc/features/csv-export.md"
            })
            .to_string(),
        )
        .await;

    assert_eq!(response.status, 200);
    assert_eq!(response.header("content-type"), Some("application/json"));
    assert_eq!(response.path(), "sdlc/features/csv-export.md");
    assert!(!dir.path().join("sdlc/features/export.md").exists());
    assert_eq!(
        fs::read_to_string(dir.path().join("sdlc/features/csv-export.md"))
            .expect("read the moved feature"),
        "---\nstatus: planned\n---\n\n# Export\n"
    );
}

#[tokio::test]
async fn move_an_epic_moves_its_tickets_and_keeps_the_number() {
    let dir = TempDir::new().expect("temp project");
    let server = TestServer::start(dir.path()).await;
    server
        .create(json!({ "kind": "epic", "title": "Foundation" }))
        .await;
    let ticket = server
        .create(json!({
            "kind": "ticket",
            "title": "Project skeleton",
            "epic": "0001-foundation"
        }))
        .await;
    assert_eq!(ticket.status, 201);
    let old_ticket_path = ticket.path();

    let response = server
        .post(
            "/api/docs/move",
            &json!({
                "from": "sdlc/epics/0001-foundation/description.md",
                "to": "sdlc/epics/0001-platform/description.md"
            })
            .to_string(),
        )
        .await;

    assert_eq!(response.status, 200);
    assert_eq!(response.path(), "sdlc/epics/0001-platform/description.md");
    let new_ticket_path = old_ticket_path.replace("0001-foundation", "0001-platform");
    assert!(dir.path().join(&new_ticket_path).is_file());
    assert!(!dir.path().join(&old_ticket_path).exists());
    assert!(!dir.path().join("sdlc/epics/0001-foundation").exists());
}

#[tokio::test]
async fn move_across_kinds_returns_400_and_moves_nothing() {
    let dir = TempDir::new().expect("temp project");
    write(
        &dir,
        "sdlc/features/export.md",
        "---\nstatus: planned\n---\n\n# Export\n",
    );
    write(
        &dir,
        "sdlc/decisions/0002-other.md",
        "---\nstatus: proposed\n---\n\n# 0002: Other\n",
    );
    let server = TestServer::start(dir.path()).await;

    let response = server
        .post(
            "/api/docs/move",
            &json!({
                "from": "sdlc/features/export.md",
                "to": "sdlc/decisions/0001-use-axum.md"
            })
            .to_string(),
        )
        .await;

    assert_error(&response, 400, "not_allowed");
    assert!(dir.path().join("sdlc/features/export.md").is_file());
    assert!(!dir.path().join("sdlc/decisions/0001-use-axum.md").exists());
}

#[tokio::test]
async fn move_rejects_a_changed_number() {
    let dir = TempDir::new().expect("temp project");
    write(
        &dir,
        "sdlc/epics/0001-foundation/description.md",
        "---\nstatus: planned\n---\n\n# 0001: Foundation\n",
    );
    let server = TestServer::start(dir.path()).await;

    let response = server
        .post(
            "/api/docs/move",
            &json!({
                "from": "sdlc/epics/0001-foundation/description.md",
                "to": "sdlc/epics/0002-platform/description.md"
            })
            .to_string(),
        )
        .await;

    assert_error(&response, 400, "not_allowed");
    assert!(
        dir.path()
            .join("sdlc/epics/0001-foundation/description.md")
            .is_file()
    );
    assert!(!dir.path().join("sdlc/epics/0002-platform").exists());
}

#[tokio::test]
async fn move_with_an_invalid_path_returns_400_before_touching_disk() {
    let dir = TempDir::new().expect("temp project");
    write(&dir, "AGENTS.md", "# Agent rules\n");
    write(
        &dir,
        "sdlc/features/export.md",
        "---\nstatus: planned\n---\n\n# Export\n",
    );
    let server = TestServer::start(dir.path()).await;

    let response = server
        .post(
            "/api/docs/move",
            &json!({
                "from": "../AGENTS.md",
                "to": "sdlc/features/moved.md"
            })
            .to_string(),
        )
        .await;

    assert_error(&response, 400, "not_allowed");
    assert_eq!(
        fs::read_to_string(dir.path().join("AGENTS.md")).expect("read agents"),
        "# Agent rules\n"
    );
    assert!(dir.path().join("sdlc/features/export.md").is_file());
}

#[tokio::test]
async fn move_a_missing_document_returns_404() {
    let dir = TempDir::new().expect("temp project");
    let server = TestServer::start(dir.path()).await;

    let response = server
        .post(
            "/api/docs/move",
            &json!({
                "from": "sdlc/features/missing.md",
                "to": "sdlc/features/other.md"
            })
            .to_string(),
        )
        .await;

    assert_error(&response, 404, "not_found");
}

#[tokio::test]
async fn delete_a_core_document_returns_400_not_allowed() {
    let dir = TempDir::new().expect("temp project");
    write(&dir, "AGENTS.md", "# Agent rules\n");
    let server = TestServer::start(dir.path()).await;

    let response = server.delete("/api/docs/AGENTS.md").await;

    assert_error(&response, 400, "not_allowed");
    assert_eq!(
        fs::read_to_string(dir.path().join("AGENTS.md")).expect("read agents"),
        "# Agent rules\n"
    );
}

#[tokio::test]
async fn delete_a_non_empty_epic_returns_409_and_removes_nothing() {
    let dir = TempDir::new().expect("temp project");
    let server = TestServer::start(dir.path()).await;
    server
        .create(json!({ "kind": "epic", "title": "Foundation" }))
        .await;
    let ticket = server
        .create(json!({
            "kind": "ticket",
            "title": "Project skeleton",
            "epic": "0001-foundation"
        }))
        .await;
    assert_eq!(ticket.status, 201);

    let response = server
        .delete("/api/docs/sdlc/epics/0001-foundation/description.md")
        .await;

    assert_error(&response, 409, "conflict");
    assert!(
        dir.path()
            .join("sdlc/epics/0001-foundation/description.md")
            .is_file()
    );
    assert!(dir.path().join(ticket.path()).is_file());
}

#[tokio::test]
async fn delete_a_feature_reports_the_orphaned_tickets() {
    let dir = TempDir::new().expect("temp project");
    let server = TestServer::start(dir.path()).await;
    server
        .create(json!({ "kind": "epic", "title": "Foundation" }))
        .await;
    server
        .create(json!({ "kind": "feature", "title": "Export" }))
        .await;
    let ticket = server
        .create(json!({
            "kind": "ticket",
            "title": "Add CSV export",
            "epic": "0001-foundation",
            "feature": "export"
        }))
        .await;
    assert_eq!(ticket.status, 201);
    let ticket_path = ticket.path();

    let response = server.delete("/api/docs/sdlc/features/export.md").await;

    assert_eq!(response.status, 200);
    assert_eq!(response.header("content-type"), Some("application/json"));
    let json = response.json();
    assert_eq!(json["orphaned_tickets"], json!(1));
    let warnings = json["warnings"]
        .as_array()
        .expect("a feature delete against a missing PRD carries index warnings");
    assert_eq!(warnings.len(), 1, "the project has no PRD yet");
    assert!(!dir.path().join("sdlc/features/export.md").exists());
    assert!(
        dir.path().join(&ticket_path).is_file(),
        "the orphaned ticket must stay"
    );
    let content = fs::read_to_string(dir.path().join(&ticket_path)).expect("read the ticket");
    assert!(content.contains("feature: export"));
}

#[tokio::test]
async fn delete_a_decision_returns_200_without_a_feature_orphan_count() {
    let dir = TempDir::new().expect("temp project");
    write(
        &dir,
        "sdlc/decisions/0001-use-axum.md",
        "---\nstatus: proposed\n---\n\n# 0001: Use axum\n",
    );
    let server = TestServer::start(dir.path()).await;

    let response = server
        .delete("/api/docs/sdlc/decisions/0001-use-axum.md")
        .await;

    assert_eq!(response.status, 200);
    assert_eq!(response.header("content-type"), Some("application/json"));
    assert_eq!(response.json(), json!({}));
    assert!(!dir.path().join("sdlc/decisions/0001-use-axum.md").exists());
}

#[tokio::test]
async fn delete_a_missing_document_returns_404() {
    let dir = TempDir::new().expect("temp project");
    let server = TestServer::start(dir.path()).await;

    let response = server.delete("/api/docs/sdlc/features/missing.md").await;

    assert_error(&response, 404, "not_found");
}

#[tokio::test]
async fn scaffold_is_idempotent_and_the_first_run_appears_in_the_tree() {
    let dir = TempDir::new().expect("temp project");
    let server = TestServer::start(dir.path()).await;

    let first = server.post("/api/scaffold", "{}").await;
    assert_eq!(first.status, 200);
    assert_eq!(first.header("content-type"), Some("application/json"));
    let first_json = first.json();
    let created = first_json["created"]
        .as_array()
        .expect("created should be an array");
    assert_eq!(created.len(), 4, "created: {first_json}");
    assert_eq!(first_json["skipped"], json!([]));

    let tree = server.get("/api/tree").await;
    assert_eq!(tree.status, 200);
    let tree_json = tree.json();
    assert_eq!(
        tree_json["docs"].as_array().expect("docs").len(),
        4,
        "tree: {tree_json}"
    );
    for path in [
        "sdlc/PRD.md",
        "sdlc/architecture.md",
        "sdlc/conventions.md",
        "AGENTS.md",
    ] {
        assert!(
            tree_paths(&tree_json).contains(&path.to_string()),
            "missing {path}"
        );
    }

    let second = server.post("/api/scaffold", "{}").await;
    assert_eq!(second.status, 200);
    let second_json = second.json();
    assert_eq!(second_json["created"], json!([]));
    assert_eq!(
        second_json["skipped"].as_array().expect("skipped").len(),
        4,
        "skipped: {second_json}"
    );
}
