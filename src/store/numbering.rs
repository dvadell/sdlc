//! Sequential number allocation (ticket 0014).
//!
//! Epics, decisions and tickets each get a four-digit number, which keeps the
//! project documents addressable and greppable. The store owns the sequence:
//! it scans the project for the highest allocated number and hands out the
//! next one, so two racing creates still end up with distinct numbers. The AI
//! never chooses a number; creation helpers ask the store for the next free
//! one and retry with a fresh number when a concurrent create won the race.

use std::path::Path;

use crate::store::layout;
use crate::store::path::DocPath;
use crate::store::{DocStore, Expectation, StoreError};

/// The last valid four-digit document number; the next allocation above it
/// fails instead of silently using five digits.
const MAX_NUMBER: u32 = 9999;

/// How many times a lost creation race is retried before giving up.
const MAX_CREATE_ATTEMPTS: usize = 3;

impl DocStore {
    /// Returns the next epic number: the highest existing epic number plus
    /// one, or `1` when the project has no epic descriptions yet.
    ///
    /// Files and directories whose names are not valid numbered documents are
    /// ignored, and gaps left behind by deleted epics push the next number
    /// higher instead of being reused.
    pub async fn next_epic_number(&self) -> Result<u32, StoreError> {
        let root = self.root.clone();
        tokio::task::spawn_blocking(move || next_epic_number_at(&root))
            .await
            .map_err(|err| StoreError::Io(std::io::Error::other(err)))?
    }

    /// Returns the next ticket number: the global highest ticket number plus
    /// one across every epic's `tickets/` directory, or `1` when there are no
    /// tickets yet.
    pub async fn next_ticket_number(&self) -> Result<u32, StoreError> {
        let root = self.root.clone();
        tokio::task::spawn_blocking(move || next_ticket_number_at(&root))
            .await
            .map_err(|err| StoreError::Io(std::io::Error::other(err)))?
    }

    /// Returns the next decision number: the highest existing decision number
    /// plus one, or `1` when the project has no decisions yet.
    pub async fn next_decision_number(&self) -> Result<u32, StoreError> {
        let root = self.root.clone();
        tokio::task::spawn_blocking(move || next_decision_number_at(&root))
            .await
            .map_err(|err| StoreError::Io(std::io::Error::other(err)))?
    }

    /// Creates a new epic description at the next epic number, retrying with a
    /// fresh number when a concurrent create already claimed it.
    ///
    /// The scan for the next number and the create run inside one write
    /// critical section, and a lost race still surfaces as
    /// [`StoreError::AlreadyExists`], which this helper answers by re-scanning
    /// for the next number instead of overwriting.
    pub async fn create_epic(&self, slug: &str, content: &str) -> Result<DocPath, StoreError> {
        let writes = self.writes.clone();
        let root = self.root.clone();
        let slug = slug.to_string();
        let content = content.to_string();

        tokio::task::spawn_blocking(move || -> Result<DocPath, StoreError> {
            let _guard = writes
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let mut last_error = None;

            for _ in 0..MAX_CREATE_ATTEMPTS {
                let number = match next_epic_number_at(&root) {
                    Ok(number) => number,
                    Err(error) => return Err(error),
                };
                let epic_name = format_epic_name(number, &slug);
                let epic_dir = root.join("sdlc").join("epics").join(&epic_name);
                std::fs::create_dir_all(&epic_dir).map_err(StoreError::Io)?;
                std::fs::create_dir_all(epic_dir.join("tickets")).map_err(StoreError::Io)?;
                let resolved = std::fs::canonicalize(epic_dir)
                    .map_err(StoreError::Io)?
                    .join("description.md");

                let path = DocPath::parse(&format!("sdlc/epics/{epic_name}/description.md"))?;
                match crate::store::save_critical(&resolved, &path, &content, &Expectation::Missing)
                {
                    Ok(_) => return Ok(path),
                    Err(StoreError::AlreadyExists(_)) => {
                        last_error = Some(StoreError::AlreadyExists(format!(
                            "sdlc/epics/{epic_name}/description.md"
                        )));
                    }
                    Err(error) => return Err(error),
                }
            }

            Err(last_error.unwrap_or(StoreError::AlreadyExists(format!(
                "sdlc/epics/{slug}/description.md"
            ))))
        })
        .await
        .map_err(|err| StoreError::Io(std::io::Error::other(err)))?
    }

    /// Creates a new decision file at the next decision number, retrying with
    /// a fresh number when a concurrent create already claimed it.
    ///
    /// The scan for the next number and the create run inside one write
    /// critical section, and a lost race still surfaces as
    /// [`StoreError::AlreadyExists`], which this helper answers by re-scanning
    /// for the next number instead of overwriting.
    pub async fn create_decision(&self, slug: &str, content: &str) -> Result<DocPath, StoreError> {
        let writes = self.writes.clone();
        let root = self.root.clone();
        let slug = slug.to_string();
        let content = content.to_string();

        tokio::task::spawn_blocking(move || -> Result<DocPath, StoreError> {
            let _guard = writes
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let mut last_error = None;

            for _ in 0..MAX_CREATE_ATTEMPTS {
                let number = match next_decision_number_at(&root) {
                    Ok(number) => number,
                    Err(error) => return Err(error),
                };
                let decision_name = format_decision_name(number, &slug);
                let path = DocPath::parse(&format!("sdlc/decisions/{decision_name}"))?;
                let parent = path.as_path().parent().map(Path::to_path_buf);
                let resolved = match parent {
                    Some(parent) => {
                        std::fs::create_dir_all(&parent).map_err(StoreError::Io)?;
                        std::fs::canonicalize(root.join(&parent))
                            .map_err(StoreError::Io)?
                            .join(path.as_path().file_name().unwrap_or_default())
                    }
                    None => {
                        return Err(StoreError::NotAllowed(path.as_path().display().to_string()));
                    }
                };
                match crate::store::save_critical(&resolved, &path, &content, &Expectation::Missing)
                {
                    Ok(_) => return Ok(path),
                    Err(StoreError::AlreadyExists(_)) => {
                        last_error = Some(StoreError::AlreadyExists(format!(
                            "sdlc/decisions/{decision_name}"
                        )));
                    }
                    Err(error) => return Err(error),
                }
            }

            Err(last_error.unwrap_or(StoreError::AlreadyExists(format!(
                "sdlc/decisions/{slug}.md"
            ))))
        })
        .await
        .map_err(|err| StoreError::Io(std::io::Error::other(err)))?
    }

    /// Creates a new ticket in an existing epic at the next global ticket
    /// number, retrying with a fresh number when a concurrent create already
    /// claimed it.
    ///
    /// The write goes through the atomic create path (`Expectation::Missing`),
    /// so a lost race surfaces as [`StoreError::AlreadyExists`] and this helper
    /// answers by re-scanning for the next number instead of overwriting. Ticket
    /// numbers are global, so the retry scans every epic's tickets.
    pub async fn create_ticket(
        &self,
        epic: &str,
        slug: &str,
        content: &str,
    ) -> Result<DocPath, StoreError> {
        let writes = self.writes.clone();
        let root = self.root.clone();
        let epic = epic.to_string();
        let slug = slug.to_string();
        let content = content.to_string();

        tokio::task::spawn_blocking(move || -> Result<DocPath, StoreError> {
            let _guard = writes
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let mut last_error = None;

            for _ in 0..MAX_CREATE_ATTEMPTS {
                let number = match next_ticket_number_at(&root) {
                    Ok(number) => number,
                    Err(error) => return Err(error),
                };
                let ticket_name = format_ticket_name(number, &slug);
                let path = DocPath::parse(&format!("sdlc/epics/{epic}/tickets/{ticket_name}"))?;
                let parent = path.as_path().parent().map(Path::to_path_buf);
                let resolved = match parent {
                    Some(parent) => {
                        std::fs::create_dir_all(&parent).map_err(StoreError::Io)?;
                        std::fs::canonicalize(root.join(&parent))
                            .map_err(StoreError::Io)?
                            .join(path.as_path().file_name().unwrap_or_default())
                    }
                    None => {
                        return Err(StoreError::NotAllowed(path.as_path().display().to_string()));
                    }
                };
                match crate::store::save_critical(&resolved, &path, &content, &Expectation::Missing)
                {
                    Ok(_) => return Ok(path),
                    Err(StoreError::AlreadyExists(_)) => {
                        last_error = Some(StoreError::AlreadyExists(format!(
                            "sdlc/epics/{epic}/tickets/{ticket_name}"
                        )));
                    }
                    Err(error) => return Err(error),
                }
            }

            Err(last_error.unwrap_or(StoreError::AlreadyExists(format!(
                "sdlc/epics/{epic}/tickets/{slug}"
            ))))
        })
        .await
        .map_err(|err| StoreError::Io(std::io::Error::other(err)))?
    }
}

/// The next epic number in the project tree: the highest existing epic number
/// plus one, or `1` when the project has no epic descriptions yet.
fn next_epic_number_at(root: &Path) -> Result<u32, StoreError> {
    let epics_dir = root.join("sdlc").join("epics");
    let Ok(entries) = std::fs::read_dir(&epics_dir) else {
        return Ok(1);
    };

    let mut highest: Option<u32> = None;
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(number) = layout::numbered_number(&name) else {
            continue;
        };
        if !entry.path().join("description.md").exists() {
            continue;
        }
        highest = Some(highest.map_or(number, |max| max.max(number)));
    }

    next_available(highest)
}

/// The next ticket number in the project tree: the global highest ticket
/// number plus one across every epic's `tickets/` directory, or `1` when no
/// ticket exists yet.
fn next_ticket_number_at(root: &Path) -> Result<u32, StoreError> {
    let epics_dir = root.join("sdlc").join("epics");
    let Ok(entries) = std::fs::read_dir(&epics_dir) else {
        return Ok(1);
    };

    let mut highest: Option<u32> = None;
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_dir() {
            continue;
        }
        let tickets_dir = entry.path().join("tickets");
        if let Some(number) = highest_numbered(&tickets_dir, true) {
            highest = Some(highest.map_or(number, |max| max.max(number)));
        }
    }

    next_available(highest)
}

/// The next decision number in the project tree: the highest existing decision
/// number plus one, or `1` when the project has no decisions yet.
fn next_decision_number_at(root: &Path) -> Result<u32, StoreError> {
    let decisions_dir = root.join("sdlc").join("decisions");
    let highest = highest_numbered(&decisions_dir, true);

    next_available(highest)
}

/// The highest valid numbered number among the document entries of `dir`,
/// ignoring entries whose names are not numbered documents and, when `file_only`
/// is set, entries that are not files.
fn highest_numbered(dir: &Path, file_only: bool) -> Option<u32> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut highest: Option<u32> = None;

    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_only && !file_type.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if let Some(number) = layout::numbered_number(&name) {
            highest = Some(highest.map_or(number, |max| max.max(number)));
        }
    }

    highest
}

/// The next free number: one above the highest allocated, or `1` when nothing
/// is allocated yet; above [`MAX_NUMBER`] the sequence is exhausted.
fn next_available(highest: Option<u32>) -> Result<u32, StoreError> {
    let next = highest.map_or(1, |highest| highest + 1);
    if next > MAX_NUMBER {
        return Err(StoreError::NumberExhausted { next });
    }

    Ok(next)
}

/// Formats an epic number and slug as the epic directory name: `0007-slug`.
pub fn format_epic_name(n: u32, slug: &str) -> String {
    format!("{n:04}-{slug}")
}

/// Formats a ticket number and slug as the ticket file name: `0007-slug.md`.
pub fn format_ticket_name(n: u32, slug: &str) -> String {
    format!("{n:04}-{slug}.md")
}

/// Formats a decision number and slug as the decision file name: `0007-slug.md`.
pub fn format_decision_name(n: u32, slug: &str) -> String {
    format!("{n:04}-{slug}.md")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    async fn store_in_temp() -> (tempfile::TempDir, DocStore) {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = DocStore::new(dir.path().to_path_buf()).expect("store");

        (dir, store)
    }

    fn write(dir: &tempfile::TempDir, relative: &str, content: &str) {
        let path = dir.path().join(relative);
        fs::create_dir_all(path.parent().expect("the parent directory")).expect("create parent");
        fs::write(path, content).expect("write the file");
    }

    #[tokio::test]
    async fn next_epic_number_is_one_for_an_empty_project() {
        let (_dir, store) = store_in_temp().await;

        assert_eq!(store.next_epic_number().await.expect("next epic"), 1);
    }

    #[tokio::test]
    async fn next_epic_number_is_the_highest_plus_one() {
        let (dir, store) = store_in_temp().await;
        write(
            &dir,
            "sdlc/epics/0001-foundation/description.md",
            "# 0001: Foundation\n",
        );
        write(
            &dir,
            "sdlc/epics/0003-platform/description.md",
            "# 0003: Platform\n",
        );

        assert_eq!(store.next_epic_number().await.expect("next epic"), 4);
    }

    #[tokio::test]
    async fn next_epic_number_ignores_unnumbered_names() {
        let (dir, store) = store_in_temp().await;
        write(
            &dir,
            "sdlc/epics/not-a-number-slug/description.md",
            "# No number\n",
        );
        write(
            &dir,
            "sdlc/epics/0002-Bad--Slug/description.md",
            "# No number\n",
        );
        fs::write(
            dir.path().join("sdlc/epics/0007-not-a-directory.md"),
            "# Not a directory\n",
        )
        .expect("write the file");

        assert_eq!(store.next_epic_number().await.expect("next epic"), 1);
    }

    #[tokio::test]
    async fn next_ticket_number_is_global_across_epics() {
        let (dir, store) = store_in_temp().await;
        write(
            &dir,
            "sdlc/epics/0001-foundation/tickets/0001-project.md",
            "# 0001: Project\n",
        );
        write(
            &dir,
            "sdlc/epics/0002-ghost/tickets/0003-late.md",
            "# 0003: Late\n",
        );

        assert_eq!(store.next_ticket_number().await.expect("next ticket"), 4);
    }

    #[tokio::test]
    async fn next_ticket_number_orders_numbers_numerically() {
        let (dir, store) = store_in_temp().await;
        write(
            &dir,
            "sdlc/epics/0001-foundation/tickets/0009-add-import.md",
            "# 0009: Add import\n",
        );
        write(
            &dir,
            "sdlc/epics/0002-platform/tickets/0010-add-export.md",
            "# 0010: Add export\n",
        );

        assert_eq!(store.next_ticket_number().await.expect("next ticket"), 11);
    }

    #[tokio::test]
    async fn next_ticket_number_ignores_invalid_names() {
        let (dir, store) = store_in_temp().await;
        write(
            &dir,
            "sdlc/epics/0001-foundation/tickets/0009-add-import.md",
            "# 0009: Add import\n",
        );
        write(
            &dir,
            "sdlc/epics/0001-foundation/tickets/not-numbered.md",
            "# Not numbered\n",
        );

        assert_eq!(store.next_ticket_number().await.expect("next ticket"), 10);
    }

    #[tokio::test]
    async fn next_decision_number_ignores_non_markdown_files() {
        let (dir, store) = store_in_temp().await;
        write(
            &dir,
            "sdlc/decisions/0002-use-axum.md",
            "# 0002: Use axum\n",
        );
        fs::write(
            dir.path().join("sdlc/decisions/0004-use-serde.txt"),
            "not markdown\n",
        )
        .expect("write the file");

        let next = store.next_decision_number().await.expect("next decision");
        assert_eq!(next, 3);
    }

    #[tokio::test]
    async fn next_number_fails_cleanly_above_the_four_digit_boundary() {
        let (dir, store) = store_in_temp().await;
        write(
            &dir,
            "sdlc/epics/9999-late/description.md",
            "# 9999: Late\n",
        );

        let error = store
            .next_epic_number()
            .await
            .expect_err("the epic sequence is exhausted");

        assert!(
            matches!(error, StoreError::NumberExhausted { next: 10000 }),
            "got {error:?}"
        );
    }

    #[tokio::test]
    async fn next_ticket_number_fails_cleanly_above_the_four_digit_boundary() {
        let (dir, store) = store_in_temp().await;
        fs::create_dir_all(dir.path().join("sdlc/epics/0001-foundation/tickets"))
            .expect("create the epic tickets directory");
        write(
            &dir,
            "sdlc/epics/0001-foundation/tickets/9999-late.md",
            "# 9999: Late\n",
        );

        let error = store
            .next_ticket_number()
            .await
            .expect_err("the ticket sequence is exhausted");

        assert!(
            matches!(error, StoreError::NumberExhausted { next: 10000 }),
            "got {error:?}"
        );
    }

    #[test]
    fn format_names_are_zero_padded_to_four_digits() {
        assert_eq!(format_epic_name(7, "add-export"), "0007-add-export");
        assert_eq!(format_ticket_name(7, "add-export"), "0007-add-export.md");
        assert_eq!(format_decision_name(7, "add-export"), "0007-add-export.md");
        assert_eq!(format_epic_name(999, "late"), "0999-late");
    }

    #[tokio::test]
    async fn create_ticket_writes_the_next_global_number() {
        let (dir, store) = store_in_temp().await;
        fs::create_dir_all(dir.path().join("sdlc/epics/0001-foundation/tickets"))
            .expect("create the epic tickets directory");

        let created = store
            .create_ticket("0001-foundation", "add-export", "# 0001: Add export\n")
            .await
            .expect("the ticket should be created");

        assert_eq!(
            created.as_path().display().to_string(),
            "sdlc/epics/0001-foundation/tickets/0001-add-export.md"
        );
        assert_eq!(
            fs::read_to_string(
                dir.path()
                    .join("sdlc/epics/0001-foundation/tickets/0001-add-export.md")
            )
            .expect("read the ticket back"),
            "# 0001: Add export\n"
        );

        let second = store
            .create_ticket("0001-foundation", "add-import", "# 0002: Add import\n")
            .await
            .expect("the second ticket should be created");
        assert_eq!(
            second.as_path().display().to_string(),
            "sdlc/epics/0001-foundation/tickets/0002-add-import.md"
        );
    }

    #[tokio::test]
    async fn two_concurrent_creates_get_distinct_numbers() {
        let (dir, store) = store_in_temp().await;
        let tasks_dir = dir.path().join("sdlc/epics/0001-foundation/tickets");
        fs::create_dir_all(&tasks_dir).expect("create the epic tickets directory");

        let task_a_store = store.clone();
        let task_a = tokio::spawn(async move {
            task_a_store
                .create_ticket("0001-foundation", "add-export", "# 0001: Add export\n")
                .await
        });
        let task_b_store = store.clone();
        let task_b = tokio::spawn(async move {
            task_b_store
                .create_ticket("0001-foundation", "add-import", "# 0002: Add import\n")
                .await
        });

        let first = task_a
            .await
            .expect("first create task")
            .expect("first create");
        let second = task_b
            .await
            .expect("second create task")
            .expect("second create");

        assert_ne!(first, second);
        let names: Vec<String> = fs::read_dir(&tasks_dir)
            .expect("read the tickets directory")
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names.len(), 2, "both creates should land: {names:?}");
        assert!(
            names.iter().any(|name| name.starts_with("0001-")),
            "the first number should be allocated: {names:?}"
        );
        assert!(
            names.iter().any(|name| name.starts_with("0002-")),
            "the second number should be allocated: {names:?}"
        );
    }
}
