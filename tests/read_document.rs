//! Integration tests for reading documents with content hashes (ticket 0010).

use std::fs;

use sdlc::store::{DocPath, DocStore, content_hash};
use tempfile::TempDir;

#[tokio::test]
async fn read_returns_a_utf8_bom_unchanged() {
    let root = TempDir::new().expect("a temporary project directory");
    let bytes = b"\xef\xbb\xbf# PRD\n";
    fs::create_dir_all(root.path().join("sdlc")).expect("sdlc directory");
    fs::write(root.path().join("sdlc/PRD.md"), bytes).expect("write test file");

    let store = DocStore::new(root.path().to_path_buf()).expect("the store should build");
    let path = DocPath::parse("sdlc/PRD.md").expect("the test path should be valid");

    let document = store.read(&path).await.expect("read the document");

    assert_eq!(document.path, path);
    assert_eq!(document.content.as_bytes(), bytes);
    assert_eq!(document.hash, content_hash(bytes));
}
