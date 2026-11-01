//! Integration tests for symlink-safe path resolution (ticket 0009).

use std::fs;
use std::path::{Path, PathBuf};

use sdlc::store::{DocPath, DocStore, ResolveMode, StoreError};
use tempfile::TempDir;

fn doc(raw: &str) -> DocPath {
    DocPath::parse(raw).expect("the test path should be a valid DocPath")
}

fn project() -> TempDir {
    TempDir::new().expect("a temporary project directory")
}

fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().expect("canonical path")
}

fn store_for(root: &Path) -> DocStore {
    DocStore::new(root.to_path_buf()).expect("the store root should canonicalize")
}

fn write(root: &Path, relative: &str, content: &str) {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("parent directories");
    }
    fs::write(path, content).expect("write test file");
}

#[tokio::test]
async fn normal_file_inside_root_resolves() {
    let root = project();
    write(root.path(), "sdlc/features/export.md", "# Export\n");
    let store = store_for(root.path());

    let resolved = store
        .resolve(&doc("sdlc/features/export.md"), ResolveMode::Existing)
        .await
        .expect("an existing file inside the root should resolve");

    assert_eq!(
        resolved,
        canonical(root.path()).join("sdlc/features/export.md")
    );
}

#[tokio::test]
async fn new_file_under_existing_inside_parent_resolves() {
    let root = project();
    fs::create_dir_all(root.path().join("sdlc/features")).expect("feature directory");
    let store = store_for(root.path());

    let resolved = store
        .resolve(
            &doc("sdlc/features/not-created-yet.md"),
            ResolveMode::NewFile,
        )
        .await
        .expect("a new file under an inside parent should resolve");

    assert_eq!(
        resolved,
        canonical(root.path()).join("sdlc/features/not-created-yet.md")
    );
    assert!(!resolved.exists(), "resolution must not create the file");
}

#[tokio::test]
async fn new_root_document_resolves_against_the_root() {
    let root = project();
    let store = store_for(root.path());

    let resolved = store
        .resolve(&doc("AGENTS.md"), ResolveMode::NewFile)
        .await
        .expect("a new root document should resolve against the root");

    assert_eq!(resolved, canonical(root.path()).join("AGENTS.md"));
}

#[tokio::test]
async fn missing_parent_returns_not_found_without_leaking_the_root() {
    let root = project();
    let store = store_for(root.path());

    let err = store
        .resolve(&doc("sdlc/features/new.md"), ResolveMode::NewFile)
        .await
        .expect_err("a missing parent must not resolve");

    assert!(matches!(&err, StoreError::NotFound(_)), "got {err:?}");
    assert!(
        !err.to_string()
            .contains(&canonical(root.path()).display().to_string()),
        "the error must not leak the absolute project root: {err}"
    );
}

#[tokio::test]
async fn missing_existing_file_returns_not_found() {
    let root = project();
    let store = store_for(root.path());

    let err = store
        .resolve(&doc("sdlc/PRD.md"), ResolveMode::Existing)
        .await
        .expect_err("a missing existing file must not resolve");

    assert!(matches!(&err, StoreError::NotFound(_)), "got {err:?}");
}

#[cfg(unix)]
mod unix {
    use super::*;
    use std::os::unix::fs::symlink;

    #[tokio::test]
    async fn symlinked_file_pointing_outside_root_is_rejected() {
        let root = project();
        let outside = project();
        write(outside.path(), "secret.md", "secret\n");
        fs::create_dir_all(root.path().join("sdlc/features")).expect("feature directory");
        symlink(
            outside.path().join("secret.md"),
            root.path().join("sdlc/features/export.md"),
        )
        .expect("file symlink");

        let store = store_for(root.path());
        let err = store
            .resolve(&doc("sdlc/features/export.md"), ResolveMode::Existing)
            .await
            .expect_err("a symlinked file outside the root must not resolve");

        assert!(matches!(&err, StoreError::NotAllowed(_)), "got {err:?}");
    }

    #[tokio::test]
    async fn symlinked_directory_pointing_outside_root_is_rejected() {
        let root = project();
        let outside = project();
        write(outside.path(), "export.md", "# Export\n");
        fs::create_dir_all(root.path().join("sdlc")).expect("sdlc directory");
        symlink(outside.path(), root.path().join("sdlc/features")).expect("directory symlink");

        let store = store_for(root.path());
        let err = store
            .resolve(&doc("sdlc/features/export.md"), ResolveMode::Existing)
            .await
            .expect_err("a symlinked directory outside the root must not resolve");

        assert!(matches!(&err, StoreError::NotAllowed(_)), "got {err:?}");
    }

    #[tokio::test]
    async fn symlink_that_stays_inside_root_is_allowed() {
        let root = project();
        write(root.path(), "sdlc/PRD.md", "# PRD\n");
        fs::create_dir_all(root.path().join("sdlc/features")).expect("feature directory");
        symlink("../PRD.md", root.path().join("sdlc/features/export.md")).expect("inside symlink");

        let store = store_for(root.path());
        let resolved = store
            .resolve(&doc("sdlc/features/export.md"), ResolveMode::Existing)
            .await
            .expect("a symlink that stays inside the root should resolve");

        assert_eq!(resolved, canonical(root.path()).join("sdlc/PRD.md"));
    }

    #[tokio::test]
    async fn symlink_chain_escaping_root_is_rejected() {
        let root = project();
        let outside = project();
        write(outside.path(), "secret.md", "secret\n");
        fs::create_dir_all(root.path().join("sdlc/features")).expect("feature directory");
        symlink(
            outside.path().join("secret.md"),
            root.path().join("sdlc/features/intermediate"),
        )
        .expect("outside symlink");
        symlink("intermediate", root.path().join("sdlc/features/export.md"))
            .expect("chain symlink");

        let store = store_for(root.path());
        let err = store
            .resolve(&doc("sdlc/features/export.md"), ResolveMode::Existing)
            .await
            .expect_err("a symlink chain escaping the root must not resolve");

        assert!(matches!(&err, StoreError::NotAllowed(_)), "got {err:?}");
    }

    #[tokio::test]
    async fn new_file_under_escaping_directory_is_rejected() {
        let root = project();
        let outside = project();
        fs::create_dir_all(root.path().join("sdlc")).expect("sdlc directory");
        symlink(outside.path(), root.path().join("sdlc/features")).expect("directory symlink");

        let store = store_for(root.path());
        let err = store
            .resolve(&doc("sdlc/features/new.md"), ResolveMode::NewFile)
            .await
            .expect_err("a new file under an escaping directory must not resolve");

        assert!(matches!(&err, StoreError::NotAllowed(_)), "got {err:?}");
    }

    #[tokio::test]
    async fn root_reached_through_symlink_uses_the_canonical_root() {
        let root = project();
        write(root.path(), "sdlc/PRD.md", "# PRD\n");
        let links = project();
        let alias = links.path().join("project-link");
        symlink(root.path(), &alias).expect("root symlink");

        let store = store_for(&alias);
        let resolved = store
            .resolve(&doc("sdlc/PRD.md"), ResolveMode::Existing)
            .await
            .expect("a file under a symlinked root should resolve");

        assert_eq!(resolved, canonical(root.path()).join("sdlc/PRD.md"));
    }
}
