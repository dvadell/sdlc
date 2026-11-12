//! Integration tests for the file watcher and event hub (tickets 0054 and 0055).

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use sdlc::chat::Block;
use sdlc::proposal::ProposalService;
use sdlc::store::{DocPath, DocStore, Expectation, RecentWrites, content_hash};
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
///
/// The hub is wired to the recent writes of the store's own instance, so the
/// watcher of a store suppresses the writes that store performs (ticket 0055).
async fn start_watcher(dir: &TempDir) -> (EventHub, tokio::task::JoinHandle<()>, DocStore) {
    let store = DocStore::new(dir.path().to_path_buf()).expect("the store should build");
    let hub = EventHub::with_recent_writes(store.recent_writes());
    let watcher = sdlc::watch::start(dir.path(), &hub)
        .await
        .expect("the watcher should start");

    tokio::time::sleep(WATCH_READY).await;

    (hub, watcher, store)
}

/// Waits past the debounce window and asserts the hub holds no event (ticket 0055).
async fn assert_no_event(receiver: &mut tokio::sync::broadcast::Receiver<FileChanged>) {
    tokio::time::sleep(Duration::from_millis(700)).await;
    assert!(
        matches!(receiver.try_recv(), Err(TryRecvError::Empty)),
        "the debounced event of the app's own write must be suppressed"
    );
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
    let (hub, watcher, _store) = start_watcher(&dir).await;
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
    let (hub, watcher, _store) = start_watcher(&dir).await;
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
    let (hub, watcher, _store) = start_watcher(&dir).await;
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
    let (hub, watcher, _store) = start_watcher(&dir).await;
    let mut receiver = hub.subscribe();

    write(&dir, "Cargo.toml", "[package]\n");
    write(&dir, "target/debug/sdlc", "binary");

    // Wait past the debounce window so the watcher has flushed its batch before
    // the empty queue is read as a filtered result.
    tokio::time::sleep(Duration::from_millis(400)).await;

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
    let (hub, watcher, _store) = start_watcher(&dir).await;
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
    let (hub, watcher, _store) = start_watcher(&dir).await;
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
    let hub = EventHub::with_recent_writes(Arc::new(RecentWrites::new()));

    sdlc::watch::start(Path::new("/definitely/missing/sdlc"), &hub)
        .await
        .expect_err("watching a missing root should fail");
}

#[tokio::test]
async fn an_app_write_is_suppressed_by_the_recent_writes() {
    let dir = TempDir::new().expect("temp project");
    std::fs::create_dir_all(dir.path().join("sdlc")).expect("create parent");
    std::fs::write(dir.path().join("sdlc/PRD.md"), "# PRD\n").expect("seed the document");
    let (hub, watcher, store) = start_watcher(&dir).await;
    let mut receiver = hub.subscribe();

    let path = DocPath::parse("sdlc/PRD.md").expect("valid path");
    let expectation = Expectation::Existing(content_hash(b"# PRD\n"));
    store
        .write(&path, "# Fresh\n", &expectation)
        .await
        .expect("the save should succeed");

    assert_no_event(&mut receiver).await;

    watcher.abort();
}

#[tokio::test]
async fn an_external_write_is_published() {
    let dir = TempDir::new().expect("temp project");
    std::fs::create_dir_all(dir.path().join("sdlc")).expect("create parent");
    std::fs::write(dir.path().join("sdlc/PRD.md"), "# PRD\n").expect("seed the document");
    let (hub, watcher, _store) = start_watcher(&dir).await;
    let mut receiver = hub.subscribe();

    write(&dir, "sdlc/PRD.md", "# External\n");

    let event = receive(&mut receiver).await;
    assert_eq!(event.path, "sdlc/PRD.md");

    watcher.abort();
}

#[tokio::test]
async fn an_app_delete_is_suppressed() {
    let dir = TempDir::new().expect("temp project");
    std::fs::create_dir_all(dir.path().join("sdlc/features")).expect("create parent");
    std::fs::write(dir.path().join("sdlc/features/export.md"), "# Export\n")
        .expect("seed the feature");
    let (hub, watcher, store) = start_watcher(&dir).await;
    let mut receiver = hub.subscribe();

    let path = DocPath::parse("sdlc/features/export.md").expect("valid path");
    let expectation = Expectation::Existing(content_hash(b"# Export\n"));
    store
        .write(&path, "# Fresh\n", &expectation)
        .await
        .expect("the save should succeed");
    store
        .delete(&path)
        .await
        .expect("the delete should succeed");

    assert_no_event(&mut receiver).await;

    watcher.abort();
}

#[tokio::test]
async fn an_external_delete_is_published() {
    let dir = TempDir::new().expect("temp project");
    std::fs::create_dir_all(dir.path().join("sdlc")).expect("create parent");
    std::fs::write(dir.path().join("sdlc/PRD.md"), "# PRD\n").expect("seed the document");
    let (hub, watcher, _store) = start_watcher(&dir).await;
    let mut receiver = hub.subscribe();

    std::fs::remove_file(dir.path().join("sdlc/PRD.md")).expect("remove the document");

    let event = receive(&mut receiver).await;
    assert_eq!(event.path, "sdlc/PRD.md");

    watcher.abort();
}

#[tokio::test]
async fn an_app_rename_is_suppressed() {
    let dir = TempDir::new().expect("temp project");
    std::fs::create_dir_all(dir.path().join("sdlc/features")).expect("create parent");
    std::fs::write(dir.path().join("sdlc/features/export.md"), "# Export\n")
        .expect("seed the feature");
    let (hub, watcher, store) = start_watcher(&dir).await;
    let mut receiver = hub.subscribe();

    let from = DocPath::parse("sdlc/features/export.md").expect("valid from");
    let to = DocPath::parse("sdlc/features/v2-api.md").expect("valid to");
    store
        .rename(&from, &to)
        .await
        .expect("the rename should succeed");

    assert_no_event(&mut receiver).await;

    watcher.abort();
}

#[tokio::test]
async fn an_external_edit_right_after_an_app_write_is_published() {
    let dir = TempDir::new().expect("temp project");
    std::fs::create_dir_all(dir.path().join("sdlc")).expect("create parent");
    std::fs::write(dir.path().join("sdlc/PRD.md"), "# PRD\n").expect("seed the document");
    let (hub, watcher, store) = start_watcher(&dir).await;
    let mut receiver = hub.subscribe();

    let path = DocPath::parse("sdlc/PRD.md").expect("valid path");
    let expectation = Expectation::Existing(content_hash(b"# PRD\n"));
    store
        .write(&path, "# Fresh\n", &expectation)
        .await
        .expect("the save should succeed");
    assert_no_event(&mut receiver).await;

    // The external edit lands right after the app write and must still notify.
    write(&dir, "sdlc/PRD.md", "# External\n");
    let event = receive(&mut receiver).await;
    assert_eq!(event.path, "sdlc/PRD.md");

    watcher.abort();
}

#[tokio::test]
async fn accepting_a_proposal_is_silent_for_every_written_file() {
    let dir = TempDir::new().expect("temp project");
    std::fs::create_dir_all(dir.path().join("sdlc/features")).expect("create parent");
    std::fs::write(dir.path().join("sdlc/features/export.md"), "# Export\n")
        .expect("seed the feature");
    let (hub, watcher, store) = start_watcher(&dir).await;
    let proposals = ProposalService::new(Arc::new(store));
    let proposal = proposals
        .create(
            &[Block::Write {
                path: "sdlc/features/export.md".to_string(),
                content: "# Exported\n".to_string(),
            }],
            None,
        )
        .await
        .expect("the proposal should build");
    proposals
        .accept(&proposal.id)
        .await
        .expect("the accept should succeed");

    let mut receiver = hub.subscribe();
    assert_no_event(&mut receiver).await;

    watcher.abort();
}
