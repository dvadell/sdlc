//! Starter layout scaffolding (ticket 0017).
//!
//! The store can create the conventional document layout from built-in
//! templates on request. Scaffolding is never automatic: the empty-layout
//! onboarding (tickets 0020 and 0026) offers it explicitly, and an existing file
//! is always skipped so a project that already has custom documents keeps them
//! byte-for-byte.

use std::path::{Path, PathBuf};

use crate::store::{DocPath, DocStore, Expectation, StoreError};

/// The report of a scaffold run: which starter files were created and which
/// were already there and therefore skipped, never overwritten.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScaffoldReport {
    /// The starter files that did not exist and were created.
    pub created: Vec<String>,
    /// The starter files that already existed and were skipped.
    pub skipped: Vec<String>,
}

impl DocStore {
    /// Creates the starter layout from the built-in templates (ticket 0017).
    ///
    /// Every starter file is written through the missing-expectation path, so
    /// an existing file is reported as skipped instead of failing or clobbering
    /// it; only genuinely missing files are created. Directories are created on
    /// demand. The run is idempotent, and the scaffold creates the three starter
    /// top-level directories plus the core documents, but no epic, ticket or
    /// fake document, so an empty project never gains a fabricated epic.
    pub async fn scaffold(&self) -> Result<ScaffoldReport, StoreError> {
        let writes = self.writes.clone();
        let root = self.root.clone();
        let files = starter_files();

        tokio::task::spawn_blocking(move || -> Result<ScaffoldReport, StoreError> {
            let _guard = writes
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let mut report = ScaffoldReport::default();

            for (raw, content) in files {
                let path =
                    DocPath::parse(raw).map_err(|_| StoreError::NotAllowed(raw.to_string()))?;
                let resolved = resolve_scaffold_path(&root, &path)?;

                match crate::store::save_critical(&resolved, &path, &content, &Expectation::Missing)
                {
                    Ok(_) => report.created.push(raw.to_string()),
                    Err(StoreError::AlreadyExists(_)) => report.skipped.push(raw.to_string()),
                    Err(error) => return Err(error),
                }
            }

            for directory in STARTER_DIRECTORIES {
                std::fs::create_dir_all(root.join(directory)).map_err(StoreError::Io)?;
            }

            report.created.sort();
            report.skipped.sort();
            Ok(report)
        })
        .await
        .map_err(|err| StoreError::Io(std::io::Error::other(err)))?
    }
}

/// The starter directories of a scaffolded project, relative to the project
/// root, created on demand when missing.
const STARTER_DIRECTORIES: [&str; 3] = ["sdlc/epics", "sdlc/decisions", "sdlc/features"];

/// The ordered starter files of a scaffolded project: the four core documents
/// with their built-in contents, and nothing else, so no epic, ticket or fake
/// document is ever scaffolded.
fn starter_files() -> Vec<(&'static str, String)> {
    vec![
        ("sdlc/PRD.md", crate::store::templates::prd_content()),
        (
            "sdlc/architecture.md",
            crate::store::templates::architecture_content(),
        ),
        (
            "sdlc/conventions.md",
            crate::store::templates::conventions_content(),
        ),
        ("AGENTS.md", crate::store::templates::agents_content()),
    ]
}

/// Resolves a starter file to its absolute path inside the root, creating the
/// parent directories on demand. The file itself is not created here; the
/// missing-expectation write decides whether it already exists.
fn resolve_scaffold_path(root: &Path, owned: &DocPath) -> Result<PathBuf, StoreError> {
    let relative = owned.as_path();
    let parent = relative.parent().map(Path::to_path_buf);
    let file_name = relative
        .file_name()
        .ok_or_else(|| crate::store::not_allowed(relative))?;

    let resolved_parent = match parent {
        Some(parent) if !parent.as_os_str().is_empty() => {
            let joined = root.join(&parent);
            std::fs::create_dir_all(&joined).map_err(StoreError::Io)?;
            std::fs::canonicalize(&joined)
                .map_err(|err| crate::store::map_resolution_error(owned, err))?
        }
        _ => root.to_path_buf(),
    };

    Ok(resolved_parent.join(file_name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    async fn store_in_temp() -> (TempDir, DocStore) {
        let dir = TempDir::new().expect("tempdir");
        let store = DocStore::new(dir.path().to_path_buf()).expect("store");

        (dir, store)
    }

    #[tokio::test]
    async fn scaffold_creates_the_starter_layout_for_an_empty_project() {
        let (dir, store) = store_in_temp().await;
        let report = store.scaffold().await.expect("the scaffold should succeed");

        assert_eq!(
            report.created,
            vec![
                "AGENTS.md".to_string(),
                "sdlc/PRD.md".to_string(),
                "sdlc/architecture.md".to_string(),
                "sdlc/conventions.md".to_string(),
            ],
            "created: {:?}",
            report.created
        );
        assert!(report.skipped.is_empty());

        for raw in [
            "sdlc/PRD.md",
            "sdlc/architecture.md",
            "sdlc/conventions.md",
            "AGENTS.md",
        ] {
            assert!(
                dir.path().join(raw).is_file(),
                "the scaffolded file {raw} should exist"
            );
        }

        assert!(dir.path().join("sdlc/epics").is_dir());
        assert!(dir.path().join("sdlc/decisions").is_dir());
        assert!(dir.path().join("sdlc/features").is_dir());
        // No fake epic and no scaffolded ticket directory.
        assert!(
            !dir.path().join("sdlc/tickets").exists(),
            "a scaffold must not create sdlc/tickets"
        );
        let epic_entries = std::fs::read_dir(dir.path().join("sdlc/epics"))
            .expect("read the epics directory")
            .flatten()
            .count();
        assert_eq!(epic_entries, 0, "a scaffold must not create a fake epic");
    }

    #[tokio::test]
    async fn scaffold_second_call_creates_nothing_and_skips_the_existing_files() {
        let (_dir, store) = store_in_temp().await;
        let first = store
            .scaffold()
            .await
            .expect("the first scaffold should succeed");
        assert_eq!(first.created.len(), 4);

        let second = store
            .scaffold()
            .await
            .expect("the second scaffold should succeed");
        assert!(second.created.is_empty(), "created: {:?}", second.created);
        assert_eq!(second.skipped.len(), 4, "skipped: {:?}", second.skipped);
        assert!(second.skipped.iter().any(|path| path == "sdlc/PRD.md"));
        assert!(second.skipped.iter().any(|path| path == "AGENTS.md"));

        let third = store
            .scaffold()
            .await
            .expect("the third scaffold should succeed");
        assert!(third.created.is_empty());
        assert_eq!(third.skipped.len(), 4);
    }

    #[tokio::test]
    async fn scaffold_partial_layout_only_fills_the_gaps() {
        let (dir, store) = store_in_temp().await;
        let custom_prd = "# My custom project\n\nPurpose.\n";
        std::fs::create_dir_all(dir.path().join("sdlc")).expect("sdlc dir");
        std::fs::write(dir.path().join("sdlc/PRD.md"), custom_prd).expect("write the custom prd");

        let report = store.scaffold().await.expect("the scaffold should succeed");

        assert_eq!(report.skipped, vec!["sdlc/PRD.md".to_string()]);
        assert_eq!(report.created.len(), 3, "created: {:?}", report.created);
        for raw in ["sdlc/architecture.md", "sdlc/conventions.md", "AGENTS.md"] {
            assert!(
                report.created.iter().any(|path| path == raw),
                "the scaffold should create the missing {raw}: created: {:?}",
                report.created
            );
        }
        assert_eq!(
            std::fs::read_to_string(dir.path().join("sdlc/PRD.md")).expect("read the prd"),
            custom_prd
        );
    }

    #[tokio::test]
    async fn scaffold_read_only_project_directory_returns_a_clear_io_error() {
        use std::os::unix::fs::PermissionsExt;

        let (dir, store) = store_in_temp().await;
        let permissions = std::fs::metadata(dir.path())
            .expect("project metadata")
            .permissions();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o500))
            .expect("lock the project directory");

        let error = store
            .scaffold()
            .await
            .expect_err("a read-only project must fail");

        std::fs::set_permissions(dir.path(), permissions).expect("restore the permissions");
        assert!(matches!(error, StoreError::Io(_)), "got {error:?}");
    }
}
