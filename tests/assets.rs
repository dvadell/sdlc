//! Integration tests for the embedded frontend asset route (ticket 0005).

use std::net::SocketAddr;
use std::path::PathBuf;

use sdlc::config::{CliArgs, Config};
use sdlc::serve;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

fn asset_config(port: u16) -> Config {
    Config::from_parts(
        CliArgs {
            dir: PathBuf::from("/tmp"),
            port,
            open: false,
        },
        |_| None,
    )
    .expect("asset config should build")
}

/// Performs a request against a running server and returns the status, content
/// type and body of the response.
async fn request_response(
    addr: SocketAddr,
    method: &str,
    target: &str,
    extra_headers: &[(&str, &str)],
) -> (u16, String, String) {
    let mut stream = TcpStream::connect(addr)
        .await
        .expect("connect to the server");
    let mut request = format!(
        "{method} {target} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\n",
        addr.port()
    );
    for (name, value) in extra_headers {
        request.push_str(name);
        request.push_str(": ");
        request.push_str(value);
        request.push_str("\r\n");
    }
    request.push_str("Connection: close\r\n\r\n");
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

async fn get_response(addr: SocketAddr, method: &str, target: &str) -> (u16, String, String) {
    request_response(addr, method, target, &[]).await
}

async fn get(addr: SocketAddr, target: &str) -> (u16, String, String) {
    get_response(addr, "GET", target).await
}

/// Performs a JSON POST so the guard lets it through to the asset handler.
async fn post_json(addr: SocketAddr, target: &str) -> (u16, String, String) {
    request_response(
        addr,
        "POST",
        target,
        &[("Content-Type", "application/json")],
    )
    .await
}

#[tokio::test]
async fn root_returns_the_served_page() {
    let config = asset_config(0);
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("the listener should bind");
    let addr = listener.local_addr().expect("the listener address");
    let server = tokio::spawn(serve(config, listener));

    let (status, content_type, body) = get(addr, "/").await;
    assert_eq!(status, 200);
    assert_eq!(content_type, "text/html");
    assert!(body.contains("The UI is under construction"));

    server.abort();
}

#[tokio::test]
async fn style_css_returns_the_styles() {
    let config = asset_config(0);
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("the listener should bind");
    let addr = listener.local_addr().expect("the listener address");
    let server = tokio::spawn(serve(config, listener));

    let (status, content_type, body) = get(addr, "/style.css").await;
    let frontend = std::fs::read_to_string("frontend/style.css").expect("read style.css");

    assert_eq!(status, 200);
    assert_eq!(content_type, "text/css");
    assert_eq!(body, frontend);

    server.abort();
}

#[tokio::test]
async fn unknown_path_falls_back_to_the_entry_page() {
    let config = asset_config(0);
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("the listener should bind");
    let addr = listener.local_addr().expect("the listener address");
    let server = tokio::spawn(serve(config, listener));

    let (status, content_type, body) = get(addr, "/anything-unknown").await;
    assert_eq!(status, 200);
    assert_eq!(content_type, "text/html");
    assert!(body.contains("The UI is under construction"));

    server.abort();
}

#[tokio::test]
async fn api_unknown_returns_json_404() {
    let config = asset_config(0);
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("the listener should bind");
    let addr = listener.local_addr().expect("the listener address");
    let server = tokio::spawn(serve(config, listener));

    let (status, content_type, body) = get(addr, "/api/unknown").await;

    assert_eq!(status, 404);
    assert_eq!(content_type, "application/json");
    let json: serde_json::Value = serde_json::from_str(&body).expect("the body should be JSON");
    assert_eq!(json["error"]["code"].as_str(), Some("not_found"));

    server.abort();
}

#[tokio::test]
async fn post_to_root_is_rejected() {
    let config = asset_config(0);
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("the listener should bind");
    let addr = listener.local_addr().expect("the listener address");
    let server = tokio::spawn(serve(config, listener));

    let (status, _, body) = post_json(addr, "/").await;
    assert_eq!(status, 405);
    assert!(body.is_empty());

    server.abort();
}

#[tokio::test]
async fn encoded_traversal_cannot_read_outside_the_frontend() {
    let config = asset_config(0);
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("the listener should bind");
    let addr = listener.local_addr().expect("the listener address");
    let server = tokio::spawn(serve(config, listener));

    let (status, _, body) = get(addr, "/%2e%2e/%2e%2e/Cargo.toml").await;

    assert_eq!(status, 404);
    assert!(!body.contains("fn main"));

    server.abort();
}
