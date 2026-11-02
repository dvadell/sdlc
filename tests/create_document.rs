//! Integration tests for creating documents from templates (ticket 0015).

use std::fs;

use sdlc::store::{DocKind, DocStore, content_hash};
use tempfile::TempDir;

/// Creates a store rooted in a fresh temporary project directory.
fn store_for() -> (TempDir, DocStore) {
    let root = TempDir::new().expect("a temporary project directory");
    let store = DocStore::new(root.path().to_path_buf()).expect("the store should build");

    (root, store)
}

#[tokio::test]
async fn create_then_read_round_trips_the_content_hash() {
    let (root, store) = store_for();

    let path = store
        .create(DocKind::Epic, "Document store", None, None)
        .await
        .expect("the epic should be created");

    let document = store.read(&path).await.expect("read the epic back");

    assert_eq!(document.hash, content_hash(document.content.as_bytes()));
    assert!(document.content.ends_with('\n'));
    assert!(!document.content.ends_with("\n\n"));

    let leftovers: Vec<String> = fs::read_dir(root.path().join("sdlc/epics/0001-document-store"))
        .expect("read the epic directory")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.contains(".tmp-"))
        .collect();

    assert!(leftovers.is_empty(), "leftover temp files: {leftovers:?}");
}

#[tokio::test]
async fn create_ticket_then_read_it_back_and_check_the_hash() {
    let (root, store) = store_for();

    std::fs::create_dir_all(root.path().join("sdlc/epics/0001-foundation/tickets"))
        .expect("the epic tickets directory");
    std::fs::create_dir_all(root.path().join("sdlc/epics/0001-foundation"))
        .expect("the epic directory");
    std::fs::create_dir_all(root.path().join("sdlc/features")).expect("the features directory");
    std::fs::write(
        root.path()
            .join("sdlc/epics/0001-foundation/description.md"),
        "---\nstatus: planned\n---\n\n# 0001: Foundation\n",
    )
    .expect("write the epic description");
    std::fs::write(
        root.path().join("sdlc/features/export.md"),
        "---\nstatus: shipped\n---\n\n# Export\n",
    )
    .expect("write the feature");

    let path = store
        .create(
            DocKind::Ticket,
            "Add CSV export",
            Some("0001-foundation"),
            Some("export"),
        )
        .await
        .expect("the ticket should be created");

    assert!(
        path.as_path()
            .to_string_lossy()
            .starts_with("sdlc/epics/0001-foundation/tickets/"),
        "the ticket should live in the epic tickets directory"
    );

    let document = store.read(&path).await.expect("read the ticket back");

    assert_eq!(document.hash, content_hash(document.content.as_bytes()));
    assert!(document.content.ends_with('\n'));
    assert!(!document.content.ends_with("\n\n"));

    let leftovers: Vec<String> =
        fs::read_dir(root.path().join("sdlc/epics/0001-foundation/tickets"))
            .expect("read the tickets directory")
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.contains(".tmp-"))
            .collect();

    assert!(leftovers.is_empty(), "leftover temp files: {leftovers:?}");
}
