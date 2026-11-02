//! Integration tests for rename and delete lifecycle actions (ticket 0016).

use std::fs;
use std::path::Path;

use sdlc::store::{DeleteReport, DocPath, DocStore, StoreError};
use tempfile::TempDir;

fn doc(raw: &str) -> DocPath {
    DocPath::parse(raw).expect("the test path should be a valid DocPath")
}

fn store_for(root: &Path) -> DocStore {
    DocStore::new(root.to_path_buf()).expect("the store should build")
}

fn write(root: &TempDir, relative: &str, content: &str) {
    let path = root.path().join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("parent directories");
    }
    fs::write(path, content).expect("write test file");
}

const FEATURE_CONTENT: &str = "---\nstatus: planned\n---\n\n# Export\n";

#[tokio::test]
async fn rename_a_feature_then_read_it_back() {
    let root = TempDir::new().expect("a temporary project directory");
    let store = store_for(root.path());
    write(&root, "sdlc/features/export.md", FEATURE_CONTENT);

    let renamed = store
        .rename(
            &doc("sdlc/features/export.md"),
            &doc("sdlc/features/csv-export.md"),
        )
        .await
        .expect("the feature should be renamed");

    assert_eq!(
        renamed.as_path().display().to_string(),
        "sdlc/features/csv-export.md"
    );
    let document = store
        .read(&renamed)
        .await
        .expect("read the renamed feature");
    assert_eq!(document.content, FEATURE_CONTENT);
}

#[tokio::test]
async fn rename_an_epic_then_read_the_moved_ticket() {
    let root = TempDir::new().expect("a temporary project directory");
    let store = store_for(root.path());
    write(
        &root,
        "sdlc/epics/0001-foundation/description.md",
        "---\nstatus: planned\n---\n\n# 0001: Foundation\n",
    );
    fs::create_dir_all(root.path().join("sdlc/epics/0001-foundation/tickets"))
        .expect("the epic tickets directory");
    write(
        &root,
        "sdlc/epics/0001-foundation/tickets/0001-project.md",
        "---\nstatus: todo\nfeature: export\n---\n\n# 0001: Project\n",
    );

    let renamed = store
        .rename(
            &doc("sdlc/epics/0001-foundation/description.md"),
            &doc("sdlc/epics/0001-platform/description.md"),
        )
        .await
        .expect("the epic should be renamed");

    assert_eq!(
        renamed.as_path().display().to_string(),
        "sdlc/epics/0001-platform/description.md"
    );

    let moved = doc("sdlc/epics/0001-platform/tickets/0001-project.md");
    let document = store.read(&moved).await.expect("read the moved ticket");
    assert_eq!(
        document.content,
        "---\nstatus: todo\nfeature: export\n---\n\n# 0001: Project\n"
    );

    assert!(
        !root.path().join("sdlc/epics/0001-foundation").exists(),
        "the old epic directory must be gone after the rename"
    );
}

#[tokio::test]
async fn rename_rejects_a_changed_number() {
    let root = TempDir::new().expect("a temporary project directory");
    let store = store_for(root.path());
    write(
        &root,
        "sdlc/epics/0001-foundation/description.md",
        "---\nstatus: planned\n---\n\n# 0001: Foundation\n",
    );

    let error = store
        .rename(
            &doc("sdlc/epics/0001-foundation/description.md"),
            &doc("sdlc/epics/0002-platform/description.md"),
        )
        .await
        .expect_err("a rename must not change the document number");

    assert!(
        matches!(error, StoreError::NumberChange { .. }),
        "got {error:?}"
    );
}

#[tokio::test]
async fn rename_rejects_cross_kind_renames() {
    let root = TempDir::new().expect("a temporary project directory");
    let store = store_for(root.path());
    write(&root, "sdlc/features/export.md", FEATURE_CONTENT);
    fs::create_dir_all(root.path().join("sdlc/decisions")).expect("decisions dir");

    let error = store
        .rename(
            &doc("sdlc/features/export.md"),
            &doc("sdlc/decisions/0001-use-axum.md"),
        )
        .await
        .expect_err("a rename may not cross kinds");

    assert!(matches!(error, StoreError::NotAllowed(_)), "got {error:?}");
}

#[tokio::test]
async fn rename_with_a_destination_collision_fails_without_overwriting() {
    let root = TempDir::new().expect("a temporary project directory");
    let store = store_for(root.path());
    write(&root, "sdlc/features/export.md", FEATURE_CONTENT);
    write(
        &root,
        "sdlc/features/csv-export.md",
        "---\nstatus: planned\n---\n\n# Csv export\n",
    );

    let error = store
        .rename(
            &doc("sdlc/features/export.md"),
            &doc("sdlc/features/csv-export.md"),
        )
        .await
        .expect_err("a destination collision must fail");

    assert!(
        matches!(error, StoreError::AlreadyExists(_)),
        "got {error:?}"
    );
    assert_eq!(
        store
            .read(&doc("sdlc/features/csv-export.md"))
            .await
            .expect("read the feature back")
            .content,
        "---\nstatus: planned\n---\n\n# Csv export\n"
    );
}

#[tokio::test]
async fn rename_a_missing_document_returns_not_found() {
    let root = TempDir::new().expect("a temporary project directory");
    let store = store_for(root.path());

    let error = store
        .rename(
            &doc("sdlc/features/nope.md"),
            &doc("sdlc/features/other.md"),
        )
        .await
        .expect_err("a rename of a missing document must fail");

    assert!(matches!(error, StoreError::NotFound(_)), "got {error:?}");
}

#[tokio::test]
async fn rename_a_core_document_is_not_allowed() {
    let root = TempDir::new().expect("a temporary project directory");
    let store = store_for(root.path());
    write(&root, "sdlc/PRD.md", "# PRD\n");

    let error = store
        .rename(&doc("sdlc/PRD.md"), &doc("sdlc/architecture.md"))
        .await
        .expect_err("a core document cannot be renamed");

    assert!(matches!(error, StoreError::NotAllowed(_)), "got {error:?}");
}

#[tokio::test]
async fn delete_a_feature_reports_the_orphaned_tickets_without_deleting_them() {
    let root = TempDir::new().expect("a temporary project directory");
    let store = store_for(root.path());
    write(
        &root,
        "sdlc/epics/0001-foundation/description.md",
        "---\nstatus: planned\n---\n\n# 0001: Foundation\n",
    );
    fs::create_dir_all(root.path().join("sdlc/epics/0001-foundation/tickets"))
        .expect("the epic tickets directory");
    write(
        &root,
        "sdlc/epics/0001-foundation/tickets/0001-project.md",
        "---\nstatus: todo\nfeature: export\n---\n\n# 0001: Project\n",
    );
    write(&root, "sdlc/features/export.md", FEATURE_CONTENT);

    let report: DeleteReport = store
        .delete(&doc("sdlc/features/export.md"))
        .await
        .expect("the feature should be deletable");

    assert_eq!(report.orphaned_tickets, 1);
    assert!(
        !root.path().join("sdlc/features/export.md").exists(),
        "the feature itself should be deleted"
    );
    assert!(
        root.path()
            .join("sdlc/epics/0001-foundation/tickets/0001-project.md")
            .exists(),
        "the orphaned ticket must not be deleted"
    );
}

#[tokio::test]
async fn delete_a_core_document_is_not_allowed() {
    let root = TempDir::new().expect("a temporary project directory");
    let store = store_for(root.path());
    write(&root, "sdlc/PRD.md", "# PRD\n");

    let error = store
        .delete(&doc("sdlc/PRD.md"))
        .await
        .expect_err("a core document cannot be deleted");

    assert!(matches!(error, StoreError::NotAllowed(_)), "got {error:?}");
}

#[tokio::test]
async fn delete_a_missing_document_returns_not_found() {
    let root = TempDir::new().expect("a temporary project directory");
    let store = store_for(root.path());

    let error = store
        .delete(&doc("sdlc/features/nope.md"))
        .await
        .expect_err("a delete of a missing document must fail");

    assert!(matches!(error, StoreError::NotFound(_)), "got {error:?}");
}

#[tokio::test]
async fn delete_a_non_empty_epic_returns_conflict_and_deletes_nothing() {
    let root = TempDir::new().expect("a temporary project directory");
    let store = store_for(root.path());
    write(
        &root,
        "sdlc/epics/0001-foundation/description.md",
        "---\nstatus: planned\n---\n\n# 0001: Foundation\n",
    );
    fs::create_dir_all(root.path().join("sdlc/epics/0001-foundation/tickets"))
        .expect("the epic tickets directory");
    write(
        &root,
        "sdlc/epics/0001-foundation/tickets/0001-project.md",
        "---\nstatus: todo\nfeature: export\n---\n\n# 0001: Project\n",
    );

    let error = store
        .delete(&doc("sdlc/epics/0001-foundation/description.md"))
        .await
        .expect_err("a non-empty epic must not be deleted");

    assert!(
        matches!(error, StoreError::EpicHasTickets { tickets: 1 }),
        "got {error:?}"
    );
    assert!(root.path().join("sdlc/epics/0001-foundation").exists());
    assert!(
        root.path()
            .join("sdlc/epics/0001-foundation/tickets/0001-project.md")
            .exists()
    );
}

#[tokio::test]
async fn delete_an_empty_epic_removes_the_directory() {
    let root = TempDir::new().expect("a temporary project directory");
    let store = store_for(root.path());
    write(
        &root,
        "sdlc/epics/0001-foundation/description.md",
        "---\nstatus: planned\n---\n\n# 0001: Foundation\n",
    );
    fs::create_dir_all(root.path().join("sdlc/epics/0001-foundation/tickets"))
        .expect("the epic tickets directory");

    let report = store
        .delete(&doc("sdlc/epics/0001-foundation/description.md"))
        .await
        .expect("the empty epic should be deletable");

    assert_eq!(report.orphaned_tickets, 0);
    assert!(!root.path().join("sdlc/epics/0001-foundation").exists());
}

#[tokio::test]
async fn delete_a_decision_or_ticket_removes_the_file() {
    let root = TempDir::new().expect("a temporary project directory");
    let store = store_for(root.path());
    write(
        &root,
        "sdlc/decisions/0001-use-axum.md",
        "---\nstatus: proposed\n---\n\n# 0001: Use axum\n",
    );

    let report = store
        .delete(&doc("sdlc/decisions/0001-use-axum.md"))
        .await
        .expect("the decision should be deletable");

    assert_eq!(report.orphaned_tickets, 0);
    assert!(!root.path().join("sdlc/decisions/0001-use-axum.md").exists());
}

#[test]
fn rename_and_delete_reject_traversal_paths() {
    for raw in [
        "../AGENTS.md",
        "sdlc/../AGENTS.md",
        "/etc/passwd",
        "sdlc/PRD.md/..",
    ] {
        assert!(
            DocPath::parse(raw).is_err(),
            "the rename and delete inputs must reject {raw:?}"
        );
    }
}

#[cfg(unix)]
mod unix {
    use super::*;
    use std::os::unix::fs::symlink;

    #[tokio::test]
    async fn rename_a_source_that_escapes_the_root_is_rejected() {
        let root = TempDir::new().expect("a temporary project directory");
        let outside = TempDir::new().expect("a temporary outside directory");
        write(&outside, "secret.md", "secret\n");
        fs::create_dir_all(root.path().join("sdlc/features")).expect("features dir");
        symlink(
            outside.path().join("secret.md"),
            root.path().join("sdlc/features/export.md"),
        )
        .expect("file symlink");

        let store = store_for(root.path());
        let error = store
            .rename(
                &doc("sdlc/features/export.md"),
                &doc("sdlc/features/export.md"),
            )
            .await
            .expect_err("a rename from outside the root must not happen");

        assert!(matches!(error, StoreError::NotAllowed(_)), "got {error:?}");
    }

    #[tokio::test]
    async fn rename_onto_a_destination_under_an_escaping_directory_is_rejected() {
        let root = TempDir::new().expect("a temporary project directory");
        let outside = TempDir::new().expect("a temporary outside directory");
        write(&outside, "export.md", "# Export\n");
        fs::create_dir_all(root.path().join("sdlc")).expect("sdlc dir");
        symlink(outside.path(), root.path().join("sdlc/features")).expect("directory symlink");
        write(&root, "sdlc/features/export.md", FEATURE_CONTENT);

        let store = store_for(root.path());
        let error = store
            .rename(
                &doc("sdlc/features/export.md"),
                &doc("sdlc/features/v2-api.md"),
            )
            .await
            .expect_err("a rename onto an escaping directory must not happen");

        assert!(matches!(error, StoreError::NotAllowed(_)), "got {error:?}");
    }

    #[tokio::test]
    async fn delete_a_source_that_escapes_the_root_is_rejected() {
        let root = TempDir::new().expect("a temporary project directory");
        let outside = TempDir::new().expect("a temporary outside directory");
        write(&outside, "secret.md", "secret\n");
        fs::create_dir_all(root.path().join("sdlc/features")).expect("features dir");
        symlink(
            outside.path().join("secret.md"),
            root.path().join("sdlc/features/export.md"),
        )
        .expect("file symlink");

        let store = store_for(root.path());
        let error = store
            .delete(&doc("sdlc/features/export.md"))
            .await
            .expect_err("a delete from outside the root must not happen");

        assert!(matches!(error, StoreError::NotAllowed(_)), "got {error:?}");
    }

    #[tokio::test]
    async fn delete_an_epic_destination_under_an_escaping_directory_is_rejected() {
        let root = TempDir::new().expect("a temporary project directory");
        let outside = TempDir::new().expect("a temporary outside directory");
        fs::create_dir_all(root.path().join("sdlc")).expect("sdlc dir");
        symlink(outside.path(), root.path().join("sdlc/epics")).expect("directory symlink");
        write(
            &root,
            "sdlc/epics/0001-foundation/description.md",
            "# 0001: Foundation\n",
        );

        let store = store_for(root.path());
        let error = store
            .delete(&doc("sdlc/epics/0001-foundation/description.md"))
            .await
            .expect_err("a delete under an escaping directory must not happen");

        assert!(matches!(error, StoreError::NotAllowed(_)), "got {error:?}");
    }
}
