//! Integration tests for scaffolding the starter layout (ticket 0017).

use sdlc::store::{CoreDoc, DocKind, DocStore};
use tempfile::TempDir;

/// Scaffolds a fresh temporary project and then lists its document tree.
async fn scaffolded_tree() -> (TempDir, sdlc::store::Tree) {
    let root = TempDir::new().expect("a temporary project directory");
    let store = DocStore::new(root.path().to_path_buf()).expect("the store should build");

    let report = store.scaffold().await.expect("the scaffold should succeed");
    assert_eq!(report.created.len(), 4, "created: {:?}", report.created);

    let tree = store.tree().await;
    assert!(tree.layout_exists);
    assert_eq!(
        tree.docs.len(),
        4,
        "the tree should list all core documents"
    );

    (root, tree)
}

#[tokio::test]
async fn scaffold_then_tree_lists_all_core_documents() {
    let (_root, tree) = scaffolded_tree().await;

    assert_eq!(tree.docs[0].kind, DocKind::Core(CoreDoc::Prd));
    assert_eq!(
        tree.docs[0].path.as_path(),
        std::path::Path::new("sdlc/PRD.md")
    );
    assert_eq!(tree.docs[1].kind, DocKind::Core(CoreDoc::Architecture));
    assert_eq!(
        tree.docs[1].path.as_path(),
        std::path::Path::new("sdlc/architecture.md")
    );
    assert_eq!(tree.docs[2].kind, DocKind::Core(CoreDoc::Conventions));
    assert_eq!(
        tree.docs[2].path.as_path(),
        std::path::Path::new("sdlc/conventions.md")
    );
    assert_eq!(tree.docs[3].kind, DocKind::Core(CoreDoc::Agents));
    assert_eq!(
        tree.docs[3].path.as_path(),
        std::path::Path::new("AGENTS.md")
    );
}
