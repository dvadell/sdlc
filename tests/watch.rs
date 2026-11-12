//! Integration tests for the file watcher and event hub (ticket 0054).

use std::path::Path;
use std::time::Duration;

use sdlc::watch::{EventHub, FileChanged};
use tempfile::TempDir;
use tokio::sync::broadcast::error::TryRecvError;
use tokio::time::timeout;

/// The debounced event of one save arrives within two seconds (ticket 0054).
const RECEIVE_TIMEOUT: Duration = Duration::from_secs(2);

/// How long the watcher needs before it reports events (ticket 0054).
const WATCH_READY: Duration = Duration::from_millis(300);

/// Writes `content` at the project-relative `relative` path of the temp project.
fn write(dir: &TempDir, relative: &str, content: &str) {
    let path = dir.path().join(relative);
    std::fs::create_dir_all(path.parent().expect("the parent directory"))
        .expect("create the parent");
    std::fs::write(path, content).expect("write the file");
}

/// Starts the watcher of a temp project and waits for it to be ready (ticket 0054).
async fn start_watcher(dir: &TempDir) -> (EventHub, tokio::task::JoinHandle<()>) {
    let hub = EventHub::new();
    let watcher = sdlc::watch::start(dir.path(), &hub)
        .await
        .expect("the watcher should start");

    tokio::time::sleep(WATCH_READY).await;

    (hub, watcher)
}

/// Receives the next event of a subscriber, failing the test past the debounce.
async fn receive(receiver: &mut tokio::sync::broadcast::Receiver<FileChanged>) -> FileChanged {
    timeout(RECEIVE_TIMEOUT, receiver.recv())
        .await
        .expect("the debounced event should arrive within 2s")
        .expect("the event should arrive")
}

#[tokio::test]
async fn writing_a_document_emits_exactly_one_event_after_the_debounce() {
    let dir = TempDir::new().expect("temp project");
    std::fs::create_dir_all(dir.path().join("sdlc")).expect("create parent");
    std::fs::write(dir.path().join("sdlc/PRD.md"), "# PRD\n").expect("seed the document");
    let (hub, watcher) = start_watcher(&dir).await;
    let mut receiver = hub.subscribe();

    write(&dir, "sdlc/PRD.md", "# PRD\n");

    let event = receive(&mut receiver).await;
    assert_eq!(event.path, "sdlc/PRD.md");
    assert!(
        matches!(receiver.try_recv(), Err(TryRecvError::Empty)),
        "one editor save must produce exactly one debounced event"
    );

    watcher.abort();
}

#[tokio::test]
async fn two_writes_within_the_debounce_window_emit_one_event() {
    let dir = TempDir::new().expect("temp project");
    std::fs::create_dir_all(dir.path().join("sdlc")).expect("create parent");
    std::fs::write(dir.path().join("sdlc/PRD.md"), "# PRD\n").expect("seed the document");
    let (hub, watcher) = start_watcher(&dir).await;
    let mut receiver = hub.subscribe();

    write(&dir, "sdlc/PRD.md", "# First\n");
    write(&dir, "sdlc/PRD.md", "# Second\n");

    let event = receive(&mut receiver).await;
    assert_eq!(event.path, "sdlc/PRD.md");
    assert!(
        matches!(receiver.try_recv(), Err(TryRecvError::Empty)),
        "two writes within the debounce window must still produce one event"
    );

    watcher.abort();
}

#[tokio::test]
async fn writes_separated_by_more_than_the_debounce_window_emit_two_events() {
    let dir = TempDir::new().expect("temp project");
    std::fs::create_dir_all(dir.path().join("sdlc")).expect("create parent");
    std::fs::write(dir.path().join("sdlc/PRD.md"), "# PRD\n").expect("seed the document");
    let (hub, watcher) = start_watcher(&dir).await;
    let mut receiver = hub.subscribe();

    write(&dir, "sdlc/PRD.md", "# First\n");
    let first = receive(&mut receiver).await;
    assert_eq!(first.path, "sdlc/PRD.md");

    // The second write lands after the 300 ms debounce window, so it is its own
    // event instead of being merged into the first one.
    tokio::time::sleep(Duration::from_millis(400)).await;
    write(&dir, "sdlc/PRD.md", "# Second\n");

    let second = receive(&mut receiver).await;
    assert_eq!(second.path, "sdlc/PRD.md");

    watcher.abort();
}

#[tokio::test]
async fn writes_outside_the_document_layout_are_ignored() {
    let dir = TempDir::new().expect("temp project");
    let (hub, watcher) = start_watcher(&dir).await;
    let mut receiver = hub.subscribe();

    write(&dir, "Cargo.toml", "[package]\n");
    write(&dir, "target/debug/sdlc", "binary");

    assert!(
        matches!(receiver.try_recv(), Err(TryRecvError::Empty)),
        "non-document files and target/ must not emit events"
    );

    watcher.abort();
}

#[tokio::test]
async fn every_subscriber_receives_the_event() {
    let dir = TempDir::new().expect("temp project");
    std::fs::create_dir_all(dir.path().join("sdlc")).expect("create parent");
    std::fs::write(dir.path().join("sdlc/PRD.md"), "# PRD\n").expect("seed the document");
    let (hub, watcher) = start_watcher(&dir).await;
    let mut first = hub.subscribe();
    let mut second = hub.subscribe();

    write(&dir, "sdlc/PRD.md", "# PRD\n");

    let first_event = receive(&mut first).await;
    assert_eq!(first_event.path, "sdlc/PRD.md");
    let second_event = receive(&mut second).await;
    assert_eq!(second_event.path, "sdlc/PRD.md");

    watcher.abort();
}

#[tokio::test]
async fn dropping_a_subscriber_does_not_stop_the_watcher() {
    let dir = TempDir::new().expect("temp project");
    std::fs::create_dir_all(dir.path().join("sdlc")).expect("create parent");
    std::fs::write(dir.path().join("sdlc/PRD.md"), "# PRD\n").expect("seed the document");
    let (hub, watcher) = start_watcher(&dir).await;
    let mut kept = hub.subscribe();
    let dropped = hub.subscribe();
    drop(dropped);

    write(&dir, "sdlc/PRD.md", "# PRD\n");

    let event = receive(&mut kept).await;
    assert_eq!(event.path, "sdlc/PRD.md");

    watcher.abort();
}

#[tokio::test]
async fn watcher_startup_failure_is_reported_for_a_missing_root() {
    let hub = EventHub::new();

    sdlc::watch::start(Path::new("/definitely/missing/sdlc"), &hub)
        .await
        .expect_err("watching a missing root should fail");
}
