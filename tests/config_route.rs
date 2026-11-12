//! Integration tests for `AppState` and `GET /api/config` (ticket 0007).

use std::net::SocketAddr;
use std::path::Path;

use sdlc::config::{CliArgs, Config};
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
    async fn start(config: Config) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("the listener should bind");
        let addr = listener.local_addr().expect("the listener address");
        let handle = tokio::spawn(serve(config, listener));

        Self { addr, handle }
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

/// Builds a config for `dir` with only the supplied AI environment variables.
fn test_config(dir: &Path, env: &[(&str, &str)]) -> Config {
    Config::from_parts(
        CliArgs {
            dir: dir.to_path_buf(),
            port: 0,
            open: false,
        },
        |key| {
            env.iter()
                .find_map(|(name, value)| (*name == key).then(|| (*value).to_owned()))
        },
    )
    .expect("the test config should build")
}

/// Performs a raw HTTP/1.1 GET and returns status, content type and body.
async fn get(addr: SocketAddr, target: &str) -> (u16, String, String) {
    let mut stream = TcpStream::connect(addr)
        .await
        .expect("connect to the server");
    let request = format!(
        "GET {target} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nConnection: close\r\n\r\n",
        addr.port()
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

    let status = response
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse::<u16>().ok())
        .expect("the status line should carry a numeric status");
    let (head, body) = response
        .split_once("\r\n\r\n")
        .expect("the response should have a head and a body");
    let content_type = head
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            if name.trim().eq_ignore_ascii_case("content-type") {
                Some(value.split(';').next().unwrap_or("").trim().to_string())
            } else {
                None
            }
        })
        .unwrap_or_default();

    (status, content_type, body.trim().to_string())
}

async fn get_config_json(addr: SocketAddr) -> (String, serde_json::Value) {
    let (status, content_type, body) = get(addr, "/api/config").await;
    assert_eq!(status, 200);
    assert_eq!(content_type, "application/json");
    let json = serde_json::from_str(&body).expect("the config body should be JSON");
    (body, json)
}

#[tokio::test]
async fn config_without_ai_reports_not_configured() {
    let dir = TempDir::new().expect("temp project");
    let server = TestServer::start(test_config(dir.path(), &[])).await;

    let (_, json) = get_config_json(server.addr).await;

    let expected_name = dir
        .path()
        .file_name()
        .expect("the temp dir has a name")
        .to_string_lossy()
        .into_owned();
    assert_eq!(json["project_name"].as_str(), Some(expected_name.as_str()));
    assert_eq!(json["port"].as_u64(), Some(u64::from(server.addr.port())));
    assert_eq!(json["ai_configured"].as_bool(), Some(false));
    assert!(json.get("model").is_none());
    assert!(json.get("ai_host").is_none());
    assert_eq!(json["version"].as_str(), Some(env!("CARGO_PKG_VERSION")));
}

#[tokio::test]
async fn config_with_ai_reports_model_and_host() {
    let dir = TempDir::new().expect("temp project");
    let server = TestServer::start(test_config(
        dir.path(),
        &[
            ("SDLC_AI_BASE_URL", "https://api.example.com/v1"),
            ("SDLC_AI_MODEL", "gpt-4o"),
            ("SDLC_AI_API_KEY", "sk-sentinel-do-not-leak"),
        ],
    ))
    .await;

    let (body, json) = get_config_json(server.addr).await;

    assert_eq!(json["ai_configured"].as_bool(), Some(true));
    assert_eq!(json["model"].as_str(), Some("gpt-4o"));
    assert_eq!(json["ai_host"].as_str(), Some("api.example.com"));
    assert!(!body.contains("sk-sentinel-do-not-leak"));
}

#[tokio::test]
async fn config_never_serializes_credentials_from_the_base_url() {
    let dir = TempDir::new().expect("temp project");
    let server = TestServer::start(test_config(
        dir.path(),
        &[
            ("SDLC_AI_BASE_URL", "https://user:pass@api.example.com/v1"),
            ("SDLC_AI_MODEL", "gpt-4o"),
        ],
    ))
    .await;

    let (body, json) = get_config_json(server.addr).await;

    assert_eq!(json["ai_host"].as_str(), Some("api.example.com"));
    assert!(!body.contains("user:pass"));
    assert!(!body.contains("pass@"));
}

#[tokio::test]
async fn port_zero_reports_the_actual_bound_port() {
    let dir = TempDir::new().expect("temp project");
    let server = TestServer::start(test_config(dir.path(), &[])).await;
    assert_ne!(
        server.addr.port(),
        0,
        "port 0 should resolve to a real port"
    );

    let (_, json) = get_config_json(server.addr).await;

    assert_eq!(json["port"].as_u64(), Some(u64::from(server.addr.port())));
}

#[tokio::test]
async fn config_exposes_the_prd_max_bytes_default_and_override() {
    let dir = TempDir::new().expect("temp project");
    let server = TestServer::start(test_config(dir.path(), &[])).await;

    let (_, json) = get_config_json(server.addr).await;

    assert_eq!(json["prd_max_bytes"].as_u64(), Some(12000));
}

#[tokio::test]
async fn config_exposes_the_prd_max_bytes_env_override() {
    let dir = TempDir::new().expect("temp project");
    let server = TestServer::start(test_config(dir.path(), &[("SDLC_PRD_MAX_BYTES", "5")])).await;

    let (_, json) = get_config_json(server.addr).await;

    assert_eq!(json["prd_max_bytes"].as_u64(), Some(5));
}

#[tokio::test]
async fn unknown_api_route_still_returns_the_json_404_shape() {
    let dir = TempDir::new().expect("temp project");
    let server = TestServer::start(test_config(dir.path(), &[])).await;

    let (status, content_type, body) = get(server.addr, "/api/not-a-route").await;

    assert_eq!(status, 404);
    assert_eq!(content_type, "application/json");
    let json: serde_json::Value = serde_json::from_str(&body).expect("the body should be JSON");
    assert_eq!(json["error"]["code"].as_str(), Some("not_found"));
}

#[cfg(unix)]
#[tokio::test]
async fn non_utf8_project_directory_name_is_lossily_reported() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    let root = TempDir::new().expect("temp root");
    let project = root.path().join(OsStr::from_bytes(b"proj\xffct"));
    std::fs::create_dir(&project).expect("create the non-UTF-8 project directory");

    let server = TestServer::start(test_config(&project, &[])).await;
    let (_, json) = get_config_json(server.addr).await;

    assert_eq!(json["project_name"].as_str(), Some("proj\u{fffd}ct"));
}
