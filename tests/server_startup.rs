//! Integration tests for the loopback server startup (ticket 0004).

use std::net::SocketAddr;
use std::path::PathBuf;

use sdlc::config::{CliArgs, Config};
use sdlc::{ServerError, bind, serve};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

fn startup_config(port: u16) -> Config {
    Config::from_parts(
        CliArgs {
            dir: PathBuf::from("/tmp"),
            port,
            open: false,
        },
        |_| None,
    )
    .expect("startup config should build")
}

/// Performs a minimal `GET /` against a running server and returns the status,
/// content type and body of the response.
async fn get_response(addr: SocketAddr) -> (u16, String, String) {
    let mut stream = TcpStream::connect(addr)
        .await
        .expect("connect to the server");
    stream
        .write_all(b"GET / HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
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

#[tokio::test]
async fn occupied_port_is_rejected_with_addr_in_use() {
    let blocker = TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("the blocker listener should bind");
    let port = blocker.local_addr().expect("the blocker address").port();
    let config = startup_config(port);

    let error = bind(&config)
        .await
        .expect_err("an occupied port should not bind");

    assert!(matches!(error, ServerError::AddrInUse { .. }));
    assert!(error.to_string().contains("--port"));
}

#[tokio::test]
async fn serve_on_ephemeral_port_responds_at_root() {
    let config = startup_config(0);
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("the listener should bind");
    let addr = listener.local_addr().expect("the listener address");
    assert_ne!(addr.port(), 0, "port 0 should resolve to an ephemeral port");

    let server = tokio::spawn(serve(config, listener));
    let (status, content_type, body) = get_response(addr).await;
    assert_eq!(status, 200);
    assert_eq!(content_type, "text/plain");
    assert_eq!(body, "sdlc");

    server.abort();
}
