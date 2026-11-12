//! Integration tests for the file-change SSE route (ticket 0056).

use std::net::SocketAddr;
use std::path::Path;
use std::time::Duration;

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
        let config = sdlc::config::Config::from_parts(
            sdlc::config::CliArgs {
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
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

/// Opens one SSE connection to the events route and waits for its response head.
async fn connect_and_wait_for_head(server: &TestServer) -> (TcpStream, String) {
    let mut stream = TcpStream::connect(server.addr)
        .await
        .expect("connect to the server");
    let request = format!(
        "GET /api/events HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nAccept: text/event-stream\r\n\r\n",
        server.addr.port()
    );
    stream
        .write_all(request.as_bytes())
        .await
        .expect("write the request");

    let mut buffer = String::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while !buffer.contains("HTTP/1.1 200") {
        let mut chunk = [0u8; 4096];
        let read = tokio::time::timeout_at(deadline, stream.read(&mut chunk)).await;
        match read {
            Ok(Ok(0)) | Ok(Err(_)) => panic!("the SSE response should deliver its head"),
            Ok(Ok(count)) => buffer.push_str(&String::from_utf8_lossy(&chunk[..count])),
            Err(_) => panic!("the SSE response head should arrive within 10s"),
        }
    }

    (stream, buffer)
}

/// Reads the stream until the marker shows up in the accumulated buffer, failing
/// the test past the deadline; returns whether the marker arrived in time.
async fn read_until(stream: &mut TcpStream, buffer: &mut String, marker: &str) -> bool {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while !buffer.contains(marker) {
        let mut chunk = [0u8; 4096];
        let read = tokio::time::timeout_at(deadline, stream.read(&mut chunk)).await;
        match read {
            Ok(Ok(0)) | Ok(Err(_)) => return false,
            Ok(Ok(count)) => buffer.push_str(&String::from_utf8_lossy(&chunk[..count])),
            Err(_) => return false,
        }
    }
    true
}

/// Writes `content` at the project-relative `relative` path of the temp project.
fn write(dir: &TempDir, relative: &str, content: &str) {
    let path = dir.path().join(relative);
    std::fs::create_dir_all(path.parent().expect("the parent directory"))
        .expect("create the parent");
    std::fs::write(path, content).expect("write the file");
}

#[tokio::test]
async fn the_events_route_streams_a_file_changed_event_for_an_external_write() {
    let dir = TempDir::new().expect("temp project");
    write(&dir, "sdlc/PRD.md", "# PRD\n");
    let server = TestServer::start(dir.path()).await;

    let (mut stream, mut buffer) = connect_and_wait_for_head(&server).await;
    assert!(
        buffer.to_ascii_lowercase().contains("text/event-stream"),
        "the SSE response should use text/event-stream: {buffer}"
    );
    assert!(
        buffer.to_ascii_lowercase().contains("no-cache"),
        "the SSE response should be no-cache: {buffer}"
    );
    assert!(
        buffer
            .to_ascii_lowercase()
            .contains("x-accel-buffering: no"),
        "the SSE response should disable proxy buffering: {buffer}"
    );
    assert!(
        !buffer.to_ascii_lowercase().contains("content-encoding"),
        "the SSE response should not be compressed: {buffer}"
    );

    write(&dir, "sdlc/PRD.md", "# External\n");
    assert!(
        read_until(&mut stream, &mut buffer, "sdlc/PRD.md").await,
        "the file-changed event should arrive after the external write"
    );
    assert!(
        buffer.contains("event: file-changed"),
        "the event frame should name file-changed: {buffer}"
    );
    assert!(
        buffer.contains("data: {\"path\":\"sdlc/PRD.md\"}"),
        "the event should carry the changed path: {buffer}"
    );

    drop(stream);
}

#[tokio::test]
async fn the_events_route_streams_two_file_changed_events_in_order() {
    let dir = TempDir::new().expect("temp project");
    write(&dir, "sdlc/PRD.md", "# PRD\n");
    let server = TestServer::start(dir.path()).await;

    let (mut stream, mut buffer) = connect_and_wait_for_head(&server).await;

    // The watcher needs a moment after the server start before it reports the
    // changes of the next write (ticket 0054).
    tokio::time::sleep(Duration::from_millis(300)).await;

    write(&dir, "sdlc/PRD.md", "# First\n");
    assert!(
        read_until(&mut stream, &mut buffer, "sdlc/PRD.md").await,
        "the first file-changed event should arrive"
    );
    write(&dir, "sdlc/architecture.md", "# Architecture\n");
    assert!(
        read_until(&mut stream, &mut buffer, "sdlc/architecture.md").await,
        "the second file-changed event should arrive"
    );

    let frames: Vec<&str> = buffer.split("event: file-changed").skip(1).collect();
    assert_eq!(
        frames.len(),
        2,
        "the stream should deliver the two file-changed events in order: {buffer}"
    );
    assert!(
        frames[0].contains("\"path\":\"sdlc/PRD.md\""),
        "the first event should carry the changed path: {}",
        frames[0]
    );
    assert!(
        frames[1].contains("\"path\":\"sdlc/architecture.md\""),
        "the second event should carry the changed path: {}",
        frames[1]
    );

    drop(stream);
}

#[tokio::test]
async fn a_disconnect_ends_the_stream_and_the_server_keeps_serving() {
    let dir = TempDir::new().expect("temp project");
    write(&dir, "sdlc/PRD.md", "# PRD\n");
    let server = TestServer::start(dir.path()).await;

    let (mut stream, mut buffer) = connect_and_wait_for_head(&server).await;
    assert!(read_until(&mut stream, &mut buffer, "HTTP/1.1 200").await);
    drop(stream);

    // The page may reconnect after the server or the proxy dropped the first
    // connection; a new client must be served again without the server panicking.
    let (mut stream, mut buffer) = connect_and_wait_for_head(&server).await;
    write(&dir, "sdlc/PRD.md", "# External\n");
    assert!(
        read_until(&mut stream, &mut buffer, "sdlc/PRD.md").await,
        "the reconnected client should receive the next event"
    );

    drop(stream);
}
