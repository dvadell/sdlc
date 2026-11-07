//! Sequential number allocation (ticket 0014).
//!
//! Epics, decisions and tickets each get a four-digit number, which keeps the
//! project documents addressable and greppable. The store owns the sequence:
//! it scans the project for the highest allocated number and hands out the
//! next one, so two racing creates still end up with distinct numbers. The AI
//! never chooses a number; creation helpers ask the store for the next free
//! one and retry with a fresh number when a concurrent create won the race.

use std::path::Path;

use crate::store::frontmatter::DecisionStatus;
use crate::store::layout;
use crate::store::path::DocPath;
use crate::store::{DocStore, Expectation, StoreError};

/// The last valid four-digit document number; the next allocation above it
/// fails instead of silently using five digits.
const MAX_NUMBER: u32 = 9999;

/// How many times a lost creation race is retried before giving up.
pub(crate) const MAX_CREATE_ATTEMPTS: usize = 3;

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
        let content = content.to_string();
        self.create_epic_at_next(slug.to_string(), move |_| content.clone())
            .await
    }

    /// Creates a new epic description at the next epic number, retrying with a
    /// fresh number when a concurrent create already claimed it.
    ///
    /// The scan for the next number and the create run inside one write
    /// critical section, and a lost race still surfaces as
    /// [`StoreError::AlreadyExists`], which this helper answers by re-scanning
    /// for the next number instead of overwriting. `content_of` renders the file
    /// content from the freshly allocated number, so a retry after a lost race
    /// writes content for the number that won.
    pub(crate) async fn create_epic_at_next(
        &self,
        slug: String,
        content_of: impl Fn(u32) -> String + Send + 'static,
    ) -> Result<DocPath, StoreError> {
        let writes = self.writes.clone();
        let root = self.root.clone();

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
                let path = DocPath::parse(&format!("sdlc/epics/{epic_name}/description.md"))?;
                let epic_dir = root.join("sdlc").join("epics").join(&epic_name);
                let epic_dir_existed = epic_dir.exists();
                std::fs::create_dir_all(&epic_dir).map_err(StoreError::Io)?;
                std::fs::create_dir_all(epic_dir.join("tickets")).map_err(StoreError::Io)?;
                let resolved = std::fs::canonicalize(&epic_dir)
                    .map_err(StoreError::Io)?
                    .join("description.md");
                let content = content_of(number);

                match crate::store::save_critical(&resolved, &path, &content, &Expectation::Missing)
                {
                    Ok(_) => return Ok(path),
                    Err(StoreError::AlreadyExists(_)) => {
                        last_error = Some(StoreError::AlreadyExists(format!(
                            "sdlc/epics/{epic_name}/description.md"
                        )));
                    }
                    Err(error) => {
                        // The epic directories were created by this attempt and
                        // the write failed, so drop the orphaned directories.
                        if !epic_dir_existed {
                            let _ = std::fs::remove_dir_all(&epic_dir);
                        }
                        return Err(error);
                    }
                }
            }

            Err(last_error.unwrap_or(StoreError::AlreadyExists(format!(
                "sdlc/epics/{slug}/description.md"
            ))))
        })
        .await
        .map_err(|err| StoreError::Io(std::io::Error::other(err)))?
    }

    /// Creates a new decision from a title with the ADR metadata of ticket
    /// 0031: the status (`proposed` by default, or `accepted` / `superseded`) and
    /// the optional supersedes target. The supersedes target may name a decision
    /// that does not exist yet, which is reported as a warning instead of failing
    /// the create.
    pub async fn create_decision(
        &self,
        title: &str,
        status: DecisionStatus,
        supersedes: Option<u32>,
    ) -> Result<(DocPath, Vec<String>), StoreError> {
        let trimmed = title.trim();
        if trimmed.is_empty()
            || trimmed.contains('\n')
            || trimmed.contains('\r')
            || trimmed.chars().count() > crate::store::templates::MAX_TITLE_LEN
        {
            return Err(StoreError::InvalidTitle(trimmed.to_string()));
        }
        let title = trimmed.to_string();
        let slug = crate::store::templates::slugify(&title)?;
        self.create_decision_at_next(slug, title, status, supersedes)
            .await
    }

    /// Creates a new decision file at the next decision number, retrying with
    /// a fresh number when a concurrent create already claimed it, carrying the
    /// ADR metadata of ticket 0031.
    ///
    /// The scan for the next number and the create run inside one write
    /// critical section, and a lost race still surfaces as
    /// [`StoreError::AlreadyExists`], which this helper answers by re-scanning
    /// for the next number instead of overwriting. `status` and `supersedes`
    /// render the file content from the freshly allocated number, so a retry
    /// after a lost race writes content for the number that won.
    ///
    /// A decision cannot supersede itself, which the freshly allocated number
    /// makes detectable, and the refusal happens before the file is written. A
    /// supersedes target that no existing decision carries is not a failure: the
    /// create reports it as a warning, because the target may be added later.
    pub(crate) async fn create_decision_at_next(
        &self,
        slug: String,
        title: String,
        status: DecisionStatus,
        supersedes: Option<u32>,
    ) -> Result<(DocPath, Vec<String>), StoreError> {
        let writes = self.writes.clone();
        let root = self.root.clone();

        tokio::task::spawn_blocking(move || -> Result<(DocPath, Vec<String>), StoreError> {
            let _guard = writes
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let mut last_error = None;
            if let Some(target) = supersedes {
                // Decision numbers run from 1 through MAX_NUMBER, so a pointer
                // outside that range can never name a decision; refuse before
                // allocating or writing.
                if target == 0 || target > MAX_NUMBER {
                    return Err(StoreError::InvalidSupersedes { supersedes: target });
                }
            }

            for _ in 0..MAX_CREATE_ATTEMPTS {
                let number = match next_decision_number_at(&root) {
                    Ok(number) => number,
                    Err(error) => return Err(error),
                };
                if supersedes == Some(number) {
                    // The pointer would name the decision being created, so the
                    // new decision would supersede itself; record the deterministic
                    // refusal through the retry path instead of returning early,
                    // without writing anything.
                    last_error = Some(StoreError::SelfSupersede { supersedes: number });
                    break;
                }
                let decision_name = format_decision_name(number, &slug);
                let path = DocPath::parse(&format!("sdlc/decisions/{decision_name}"))?;
                let parent = path.as_path().parent().map(Path::to_path_buf);
                let resolved = match parent {
                    Some(parent) => {
                        let rooted_parent = root.join(&parent);
                        std::fs::create_dir_all(&rooted_parent).map_err(StoreError::Io)?;
                        let file_name = path.as_path().file_name().ok_or_else(|| {
                            StoreError::NotAllowed(path.as_path().display().to_string())
                        })?;
                        std::fs::canonicalize(&rooted_parent)
                            .map_err(StoreError::Io)?
                            .join(file_name)
                    }
                    None => {
                        return Err(StoreError::NotAllowed(path.as_path().display().to_string()));
                    }
                };
                let content =
                    crate::store::templates::decision_content(number, &title, status, supersedes);
                match crate::store::save_critical(&resolved, &path, &content, &Expectation::Missing)
                {
                    Ok(_) => {
                        // The dangling-target check runs after the file is
                        // committed, so a read failure is reported as a warning
                        // here instead of failing the create that already succeeded.
                        let mut warnings = Vec::new();
                        if let Some(target) = supersedes {
                            match decision_number_exists(&root, target) {
                                Ok(true) => {}
                                Ok(false) => warnings.push(format!(
                                    "the decision {target} does not exist yet; the supersedes pointer may dangle until it is created"
                                )),
                                Err(error) => {
                                    tracing::warn!(error = %error, "could not verify the supersedes target");
                                    warnings.push(format!(
                                        "the supersedes target {target} could not be verified: {error}"
                                    ));
                                }
                            }
                        }
                        return Ok((path, warnings));
                    }
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
        let content = content.to_string();
        self.create_ticket_at_next(epic.to_string(), slug.to_string(), move |_| content.clone())
            .await
    }

    /// Creates a new ticket in an existing epic at the next global ticket
    /// number, retrying with a fresh number when a concurrent create already
    /// claimed it.
    ///
    /// The write goes through the atomic create path (`Expectation::Missing`),
    /// so a lost race surfaces as [`StoreError::AlreadyExists`] and this helper
    /// answers by re-scanning for the next number instead of overwriting. Ticket
    /// numbers are global, so the retry scans every epic's tickets.
    ///
    /// `content_of` renders the file content from the freshly allocated number,
    /// so a retry after a lost race writes content for the number that won.
    pub(crate) async fn create_ticket_at_next(
        &self,
        epic: String,
        slug: String,
        content_of: impl Fn(u32) -> String + Send + 'static,
    ) -> Result<DocPath, StoreError> {
        let writes = self.writes.clone();
        let root = self.root.clone();

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
                        let rooted_parent = root.join(&parent);
                        std::fs::create_dir_all(&rooted_parent).map_err(StoreError::Io)?;
                        let file_name = path.as_path().file_name().ok_or_else(|| {
                            StoreError::NotAllowed(path.as_path().display().to_string())
                        })?;
                        std::fs::canonicalize(&rooted_parent)
                            .map_err(StoreError::Io)?
                            .join(file_name)
                    }
                    None => {
                        return Err(StoreError::NotAllowed(path.as_path().display().to_string()));
                    }
                };
                let content = content_of(number);
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
pub(crate) fn next_epic_number_at(root: &Path) -> Result<u32, StoreError> {
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
pub(crate) fn next_ticket_number_at(root: &Path) -> Result<u32, StoreError> {
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
        if let Some(number) = highest_numbered(&tickets_dir) {
            highest = Some(highest.map_or(number, |max| max.max(number)));
        }
    }

    next_available(highest)
}

/// The next decision number in the project tree: the highest existing decision
/// number plus one, or `1` when the project has no decisions yet.
pub(crate) fn next_decision_number_at(root: &Path) -> Result<u32, StoreError> {
    let decisions_dir = root.join("sdlc").join("decisions");
    let highest = highest_numbered(&decisions_dir);

    next_available(highest)
}

/// The highest valid numbered number among the document entries of `dir`,
/// ignoring entries whose names are not numbered documents or that are not
/// files.
pub(crate) fn highest_numbered(dir: &Path) -> Option<u32> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut highest: Option<u32> = None;

    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_file() {
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
pub(crate) fn next_available(highest: Option<u32>) -> Result<u32, StoreError> {
    let next = highest.map_or(1, |highest| highest + 1);
    if next > MAX_NUMBER {
        return Err(StoreError::NumberExhausted { next });
    }

    Ok(next)
}

/// Whether the project already holds a decision with the given number, so a
/// supersedes pointer can be checked against the decision it names (ticket 0031).
///
/// A missing decisions directory means no decision exists yet, which reads as
/// an absent target; any other read failure is a real filesystem error and is
/// surfaced instead of being mistaken for an absent decision.
pub(crate) fn decision_number_exists(root: &Path, number: u32) -> Result<bool, StoreError> {
    let decisions_dir = root.join("sdlc").join("decisions");
    let entries = match std::fs::read_dir(&decisions_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(StoreError::Io(error)),
    };

    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => return Err(StoreError::Io(error)),
        };
        let file_type = entry.file_type().map_err(StoreError::Io)?;
        if !file_type.is_file() {
            continue;
        }
        if layout::numbered_number(&entry.file_name().to_string_lossy())
            .is_some_and(|existing| existing == number)
        {
            return Ok(true);
        }
    }

    Ok(false)
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

    #[tokio::test]
    async fn create_decision_writes_the_proposed_status_and_the_next_number() {
        let (dir, store) = store_in_temp().await;

        let (created, warnings) = store
            .create_decision("Use axum", DecisionStatus::Proposed, None)
            .await
            .expect("the decision should be created");

        assert_eq!(
            created.as_path().display().to_string(),
            "sdlc/decisions/0001-use-axum.md"
        );
        assert!(warnings.is_empty());

        let content = fs::read_to_string(dir.path().join("sdlc/decisions/0001-use-axum.md"))
            .expect("read the decision back");
        assert!(content.starts_with("---\nstatus: proposed\n---\n\n"));
        assert!(content.contains("# 0001: Use axum"));
    }

    #[tokio::test]
    async fn create_decision_accepts_each_decision_status() {
        let (_dir, store) = store_in_temp().await;

        let (first, first_warnings) = store
            .create_decision("Use axum", DecisionStatus::Proposed, None)
            .await
            .expect("the first decision should be created");
        assert_eq!(
            first.as_path().display().to_string(),
            "sdlc/decisions/0001-use-axum.md"
        );
        assert!(first_warnings.is_empty());

        let (second, second_warnings) = store
            .create_decision("Use Postgres", DecisionStatus::Accepted, None)
            .await
            .expect("the second decision should be created");
        assert_eq!(
            second.as_path().display().to_string(),
            "sdlc/decisions/0002-use-postgres.md"
        );
        assert!(second_warnings.is_empty());

        let (third, third_warnings) = store
            .create_decision("Use SQLite", DecisionStatus::Superseded, None)
            .await
            .expect("the third decision should be created");
        assert_eq!(
            third.as_path().display().to_string(),
            "sdlc/decisions/0003-use-sqlite.md"
        );
        assert!(third_warnings.is_empty());
    }

    #[tokio::test]
    async fn create_decision_numbers_a_document_that_follows_a_gap() {
        let (dir, store) = store_in_temp().await;
        std::fs::create_dir_all(dir.path().join("sdlc/decisions")).expect("decisions dir");
        std::fs::write(
            dir.path().join("sdlc/decisions/0007-legacy.md"),
            "---\nstatus: proposed\n---\n\n# 0007: Legacy\n",
        )
        .expect("write the decision");

        let (created, _warnings) = store
            .create_decision("Use axum", DecisionStatus::Proposed, None)
            .await
            .expect("the decision should be created");

        assert_eq!(
            created.as_path().display().to_string(),
            "sdlc/decisions/0008-use-axum.md"
        );
    }

    #[tokio::test]
    async fn create_decision_with_a_missing_supersedes_target_reports_a_warning() {
        let (dir, store) = store_in_temp().await;

        let (_created, warnings) = store
            .create_decision("Use axum", DecisionStatus::Proposed, Some(42))
            .await
            .expect("a missing supersedes target is a warning, not a failure");

        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("the decision 42 does not exist yet"));
        assert!(
            dir.path().join("sdlc/decisions/0001-use-axum.md").is_file(),
            "the decision is still created"
        );
    }

    #[tokio::test]
    async fn create_decision_refuses_a_decision_that_supersedes_itself() {
        let (_dir, store) = store_in_temp().await;

        let error = store
            .create_decision("Use axum", DecisionStatus::Proposed, Some(1))
            .await
            .expect_err("a decision cannot supersede itself");

        assert!(
            matches!(error, StoreError::SelfSupersede { supersedes: 1 }),
            "got {error:?}"
        );
    }

    #[tokio::test]
    async fn create_decision_with_a_non_positive_supersedes_target_is_refused() {
        let (_dir, store) = store_in_temp().await;

        let error = store
            .create_decision("Use axum", DecisionStatus::Proposed, Some(0))
            .await
            .expect_err("a supersedes pointer of zero can never name a decision");

        assert!(
            matches!(error, StoreError::InvalidSupersedes { supersedes: 0 }),
            "got {error:?}"
        );
    }

    #[tokio::test]
    async fn create_decision_keeps_the_supersedes_pointer_across_a_round_trip() {
        let (dir, store) = store_in_temp().await;
        let (_first, first_warnings) = store
            .create_decision("Use axum", DecisionStatus::Proposed, None)
            .await
            .expect("the first decision should be created");
        assert!(first_warnings.is_empty());
        let (second, second_warnings) = store
            .create_decision("Use Postgres", DecisionStatus::Accepted, Some(1))
            .await
            .expect("the second decision should be created");
        assert!(second_warnings.is_empty());

        let second_path = "sdlc/decisions/0002-use-postgres.md";
        let content = fs::read_to_string(dir.path().join(second_path)).expect("read the decision");
        assert!(content.starts_with("---\nstatus: accepted\nsupersedes: 1\n---\n\n"));
        assert!(content.contains("# 0002: Use Postgres"));

        let document = store.read(&second).await.expect("read the decision");
        let meta = crate::store::frontmatter::parse_decision(&document.content, second_path);
        assert_eq!(meta.status, Some(DecisionStatus::Accepted));
        assert_eq!(meta.supersedes, Some(1));

        store
            .write(&second, &content, &Expectation::Existing(document.hash))
            .await
            .expect("save the decision");

        let reread = store.read(&second).await.expect("read the decision again");
        let meta = crate::store::frontmatter::parse_decision(&reread.content, second_path);
        assert_eq!(meta.status, Some(DecisionStatus::Accepted));
        assert_eq!(meta.supersedes, Some(1));
    }
}
