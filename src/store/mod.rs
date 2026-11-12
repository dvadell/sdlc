//! The document store (tickets 0008–0017).
//!
//! The store is the only component that touches project files. This layer
//! establishes the validated [`DocPath`], the layout whitelist, symlink-safe
//! resolution, content reads with a SHA-256 hash and atomic, hash-checked
//! writes, the document tree listing, the sequential number allocation and the
//! creation of documents from templates and the scaffold of the starter layout;
//! later tickets add the PRD feature index writer.

pub mod frontmatter;
pub mod index;
pub mod layout;
pub mod numbering;
pub mod path;
pub mod scaffold;
pub mod templates;
pub mod tree;

use std::ffi::OsStr;
use std::io;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::store::index::{RewriteOutcome, rewrite};

/// The project-relative path of the PRD the feature index is rendered into.
const PRD_PATH: &str = "sdlc/PRD.md";

/// How long a recorded self-write stays comparable before it expires (ticket 0055).
const RECENT_WRITE_TTL: Duration = Duration::from_secs(5);

/// The maximum number of recorded self-writes kept before the oldest is pruned
/// (ticket 0055).
const MAX_RECENT_WRITES: usize = 1024;

/// Errors from the document store.
///
/// HTTP mapping is added by a later ticket; the store deliberately knows
/// nothing about axum.
#[derive(Debug, Error)]
pub enum StoreError {
    /// The path is not one of the allowed document paths.
    #[error("not an allowed document path: {0}")]
    NotAllowed(String),

    /// The document does not exist.
    #[error("document not found: {0}")]
    NotFound(String),

    /// A document already exists where one must not.
    #[error("document already exists: {0}")]
    AlreadyExists(String),

    /// The document changed on disk since the caller last read it.
    #[error("document changed on disk")]
    Conflict { current_hash: String },

    /// The document's bytes are not valid UTF-8.
    #[error("document is not valid UTF-8: {0}")]
    InvalidUtf8(String),

    /// The document title failed validation: blank, multi-line, over-long or
    /// not slugifiable into a file name.
    #[error("invalid document title: {0}")]
    InvalidTitle(String),

    /// The four-digit document number space is exhausted; the next allocation
    /// would need five digits.
    #[error("no document numbers remain; the next number would be {next}")]
    NumberExhausted { next: u32 },

    /// A rename would move a numbered document to another number; the number is
    /// part of the document identity and only the slug may change.
    #[error("document number cannot change from {from_name} to {to_name}")]
    NumberChange {
        /// The source document being renamed, as a number-prefixed name.
        from_name: String,
        /// The destination document the rename would have produced.
        to_name: String,
    },

    /// The epic directory still holds tickets, so it cannot be deleted yet.
    #[error("{}", epic_has_tickets_message(*tickets))]
    EpicHasTickets {
        /// The number of tickets that would move with the epic.
        tickets: usize,
    },

    /// A filesystem operation failed.
    #[error(transparent)]
    Io(#[from] std::io::Error),

    /// The supersedes pointer names the decision being created, so the new
    /// decision would supersede itself; refused before the write (ticket 0031).
    #[error("a decision cannot supersede itself: {supersedes}")]
    SelfSupersede { supersedes: u32 },

    /// The supersedes pointer names a decision number that can never exist, so
    /// the create is refused before anything is written (ticket 0031).
    #[error("the supersedes decision number is invalid: {supersedes}")]
    InvalidSupersedes { supersedes: u32 },
}

pub use layout::{CoreDoc, DocKind};
pub use numbering::{format_decision_name, format_epic_name, format_ticket_name};
pub use path::DocPath;
pub use scaffold::ScaffoldReport;
pub use tree::{DocEntry, Tree};

/// How [`DocStore::resolve`] treats the target's existence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolveMode {
    /// The document must already exist, so every path component is checked.
    Existing,
    /// The document is about to be created; only its parent must exist.
    NewFile,
}

/// A document read from the store: its content plus the content hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Document {
    /// The validated path of the document.
    pub path: DocPath,
    /// The document content, byte-for-byte as UTF-8 text.
    pub content: String,
    /// Lowercase hex SHA-256 of the content bytes.
    pub hash: String,
}

/// What the caller expects to find on disk before a save is allowed to write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expectation {
    /// The caller saw this content hash and claims the document is unchanged,
    /// so the write may replace the current content.
    Existing(String),
    /// The caller believes the document does not exist yet, so the write must
    /// create it and refuses to overwrite anything that is already there.
    Missing,
}

impl Expectation {
    /// The resolution mode that matches this expectation.
    fn mode(&self) -> ResolveMode {
        match self {
            Expectation::Existing(_) => ResolveMode::Existing,
            Expectation::Missing => ResolveMode::NewFile,
        }
    }
}

/// The report of a delete: how many tickets were left pointing at the deleted
/// feature, and therefore orphaned instead of removed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeleteReport {
    /// The number of tickets whose feature binding named the deleted feature;
    /// those tickets are not deleted or rewritten.
    pub orphaned_tickets: usize,
}

/// One write sdlc performed on a document, remembered until the watcher has
/// compared the file against it (ticket 0055).
#[derive(Debug, Clone, PartialEq, Eq)]
struct RecentWrite {
    /// The content hash the write produced; `None` marks a delete of a document
    /// that no longer exists.
    hash: Option<String>,
    /// When the write was recorded; entries expire after the grace period.
    inserted: Instant,
}

/// The recent writes of one store, behind a mutex (ticket 0055).
///
/// The store records every write it performs with the content hash that was
/// written, plus an insert timestamp, so the file watcher can tell the changes
/// sdlc made itself apart from external ones: a watcher event whose file still
/// holds the recorded hash is suppressed, and a recorded delete whose file stays
/// missing is suppressed as well. Entries expire after the grace period, so an
/// external change arriving later is never mistaken for the write that just
/// happened, and the map is bounded to [`MAX_RECENT_WRITES`] entries.
#[derive(Debug)]
pub struct RecentWrites {
    ttl: Duration,
    max: usize,
    entries: std::sync::Mutex<std::collections::HashMap<String, RecentWrite>>,
}

impl Default for RecentWrites {
    fn default() -> Self {
        Self::new()
    }
}

impl RecentWrites {
    /// Creates a recent-writes map with the default grace period and bound (ticket 0055).
    pub fn new() -> Self {
        Self::with_limits(RECENT_WRITE_TTL, MAX_RECENT_WRITES)
    }

    /// Creates a recent-writes map whose entries expire after `ttl` and are
    /// bounded to `max` entries (ticket 0055).
    pub fn with_limits(ttl: Duration, max: usize) -> Self {
        Self {
            ttl,
            max,
            entries: std::sync::Mutex::new(std::collections::HashMap::new()),
        }
    }

    /// Records the write of `path` with the content hash it produced (ticket 0055).
    ///
    /// The caller records before the write completes, because the content is
    /// known before the write runs and the watcher event cannot arrive first.
    pub fn record_write(&self, path: &str, hash: &str) {
        self.record(path, Some(hash));
    }

    /// Records the delete of `path` (ticket 0055).
    pub fn record_delete(&self, path: &str) {
        self.record(path, None);
    }

    /// Decides whether a change of `path` must be published (ticket 0055).
    ///
    /// `current_hash` is the hash of the file as the watcher read it back; `None`
    /// means the file did not read back, as after a delete. The recorded write
    /// that still matches the file suppresses the event; a recorded delete whose
    /// file stays missing suppresses it as well, and anything else is published.
    pub fn should_publish(&self, path: &str, current_hash: Option<&str>) -> bool {
        let mut entries = self.lock_entries();
        let Some(entry) = entries.get(path).cloned() else {
            // No write was recorded for the path, so the change is not ours.
            return true;
        };
        if entry.inserted.elapsed() > self.ttl {
            // The grace period elapsed, so the recorded write is stale context.
            entries.remove(path);
            return true;
        }

        let publish = match (&entry.hash, current_hash) {
            // The file still holds the content the write produced, so the watcher
            // heard sdlc's own write and the event is suppressed, not published.
            (Some(recorded), Some(current)) => current != recorded.as_str(),
            // The write deleted the document and nothing brought it back, so the
            // delete is sdlc's own and stays suppressed.
            (None, None) => false,
            // Any other combination is an external change around our write.
            _ => true,
        };

        if publish {
            // The change is not the recorded write, so the write it belongs to is
            // spent; forgetting it lets later external changes publish again.
            entries.remove(path);
        }

        publish
    }

    /// The number of recent writes currently remembered (ticket 0055).
    pub fn len(&self) -> usize {
        self.lock_entries().len()
    }

    /// Whether no recent write is remembered (ticket 0055).
    pub fn is_empty(&self) -> bool {
        self.lock_entries().is_empty()
    }

    /// Locks the recent writes map, recovering a poisoned lock (ticket 0055).
    fn lock_entries(
        &self,
    ) -> std::sync::MutexGuard<'_, std::collections::HashMap<String, RecentWrite>> {
        self.entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Records one write or delete, pruning expired entries and bounding the
    /// remembered writes (ticket 0055).
    fn record(&self, path: &str, hash: Option<&str>) {
        let mut entries = self.lock_entries();
        entries.retain(|_, entry| entry.inserted.elapsed() <= self.ttl);
        entries.insert(
            path.to_string(),
            RecentWrite {
                hash: hash.map(str::to_string),
                inserted: Instant::now(),
            },
        );
        if entries.len() > self.max {
            // Drop the oldest write, so the map cannot grow without bound.
            let oldest = entries
                .iter()
                .min_by_key(|(_, entry)| entry.inserted)
                .map(|(path, _)| path.clone());
            if let Some(oldest) = oldest {
                entries.remove(&oldest);
            }
        }
    }
}

/// The document store, rooted at one canonical project directory.
#[derive(Debug, Clone)]
pub struct DocStore {
    root: PathBuf,
    /// Serializes the check-then-write sequence of saves, so two concurrent
    /// writers cannot both pass an expectation check and rename.
    ///
    /// Invariant: a single store instance owns a single project root; create
    /// one [`DocStore`] per root and share the [`Arc`] across tasks.
    writes: std::sync::Arc<std::sync::Mutex<()>>,
    /// The recent writes of this store, which the event hub consults to suppress
    /// the events of the writes the store performs itself (ticket 0055).
    recent_writes: std::sync::Arc<RecentWrites>,
}

impl DocStore {
    /// Creates a store rooted at `root`, canonicalizing the root once.
    ///
    /// Resolution always compares against the canonical root, so a project
    /// directory that is itself reached through a symlink works normally.
    pub fn new(root: PathBuf) -> Result<Self, StoreError> {
        let root = root.canonicalize()?;
        Ok(Self {
            root,
            writes: std::sync::Arc::new(std::sync::Mutex::new(())),
            recent_writes: std::sync::Arc::new(RecentWrites::new()),
        })
    }

    /// The recent writes of this store, shared with the event hub that watches
    /// it, so a watcher event can be told apart from the store's own writes
    /// (ticket 0055).
    pub fn recent_writes(&self) -> std::sync::Arc<RecentWrites> {
        self.recent_writes.clone()
    }

    /// Resolves a validated document path to an absolute path inside the root.
    ///
    /// `Existing` canonicalizes the whole target, so a symlink in any component
    /// that points outside the root is rejected. `NewFile` canonicalizes only
    /// its parent, because the file itself does not exist yet, and then joins
    /// the final file name.
    ///
    /// # TOCTOU
    ///
    /// Resolution and the caller's later open/read/write are separate
    /// filesystem operations. A local attacker who can already write the
    /// project tree can replace a checked path with a symlink in that window.
    /// This is accepted for a local single-user tool; ticket 0011's write path
    /// inherits the same window and must not be presented as a security
    /// boundary against that attacker.
    pub async fn resolve(&self, path: &DocPath, mode: ResolveMode) -> Result<PathBuf, StoreError> {
        match mode {
            ResolveMode::Existing => {
                let joined = self.root.join(path.as_path());
                let resolved = tokio::fs::canonicalize(&joined)
                    .await
                    .map_err(|err| map_resolution_error(path, err))?;
                ensure_inside_root(&self.root, path, resolved)
            }
            ResolveMode::NewFile => {
                let relative = path.as_path();
                let parent = relative.parent().ok_or_else(|| not_allowed(relative))?;
                let file_name = relative.file_name().ok_or_else(|| not_allowed(relative))?;
                let joined_parent = self.root.join(parent);
                let resolved_parent = tokio::fs::canonicalize(&joined_parent)
                    .await
                    .map_err(|err| map_resolution_error(path, err))?;
                let resolved_parent = ensure_inside_root(&self.root, path, resolved_parent)?;

                Ok(resolved_parent.join(file_name))
            }
        }
    }

    /// Reads a document and returns its content with the content hash.
    ///
    /// The raw bytes are hashed, not the decoded string, so CRLF, BOMs and
    /// trailing whitespace survive into the hash and later conflict checks.
    /// Decoding and hashing run inside `spawn_blocking` so big files never sit
    /// on the async worker.
    pub async fn read(&self, path: &DocPath) -> Result<Document, StoreError> {
        let resolved = self.resolve(path, ResolveMode::Existing).await?;
        let owned = path.clone();

        tokio::task::spawn_blocking(move || read_document(&resolved, &owned))
            .await
            .map_err(|err| StoreError::Io(io::Error::other(err)))?
    }

    /// Atomically writes `content` and returns the hash of the bytes written.
    ///
    /// The expectation decides what the caller saw before writing:
    ///
    /// - [`Expectation::Existing`] saves only while the file on disk still
    ///   hashes to the expected value, and fails with a conflict that carries
    ///   the hash actually on disk otherwise, so a stale save never clobbers an
    ///   external change.
    /// - [`Expectation::Missing`] creates the file and fails with
    ///   [`StoreError::AlreadyExists`] when anything is already in the way.
    ///
    /// The write is atomic: the bytes go to a synced temp file that is renamed
    /// over the target, so a failure never leaves a partial document behind.
    /// The returned hash is the hash of the content that was written, ready to
    /// be sent back by the next save.
    ///
    /// The PRD feature index refresh after a saved feature is the caller's
    /// responsibility: callers that keep the index in sync invoke
    /// [`DocStore::refresh_feature_index`] themselves and surface its warnings,
    /// which is what the API layer does after a successful save.
    pub async fn write(
        &self,
        path: &DocPath,
        content: &str,
        expectation: &Expectation,
    ) -> Result<String, StoreError> {
        let resolved = self.resolve(path, expectation.mode()).await?;
        let owned = path.clone();
        let owned_content = content.to_string();
        let owned_expectation = expectation.clone();

        let written = self
            .write_resolved(resolved, owned, owned_content, owned_expectation)
            .await?;

        Ok(written)
    }

    /// Runs the atomic check-and-write sequence for one already-resolved
    /// document, holding the write lock across the check and the write.
    async fn write_resolved(
        &self,
        resolved: PathBuf,
        owned: DocPath,
        owned_content: String,
        owned_expectation: Expectation,
    ) -> Result<String, StoreError> {
        let writes = self.writes.clone();
        let recent_writes = self.recent_writes.clone();

        tokio::task::spawn_blocking(move || -> Result<String, StoreError> {
            // The expectation check and the write are one critical section, so
            // two concurrent writers cannot both pass a check and rename. The
            // guard is only held inside the blocking task, never across an
            // `.await`.
            let _guard = writes
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());

            save_critical(
                &resolved,
                &owned,
                &owned_content,
                &owned_expectation,
                &recent_writes,
            )
        })
        .await
        .map_err(|err| StoreError::Io(io::Error::other(err)))?
    }

    /// Refreshes the PRD feature index after a feature changed (ticket 0029).
    ///
    /// This is the single refresh owner for the PRD feature index: the mutating
    /// store methods deliberately do not refresh themselves, so callers that
    /// keep the index in sync invoke this and surface its warnings themselves
    /// (the API layer does, after a successful feature write, create, rename or
    /// delete).
    ///
    /// Reads every feature entry, reads `sdlc/PRD.md` when present, and lets
    /// the pure index writer render the replacement for the marked region; the
    /// PRD is written only when the rendered index differs from the current
    /// content, so an already-correct index is never rewritten.
    ///
    /// A missing PRD, missing markers or an unwritable PRD yield a warning
    /// instead of a failure: the feature operation itself already succeeded and
    /// must not be reported as failed because the index could not be updated.
    ///
    /// # TOCTOU
    ///
    /// The PRD read and write-back run inside one write critical section, so the
    /// rendered index is written through the same critical section the feature
    /// writes take; the feature list is captured before that critical section is
    /// acquired, so the directory traversal and the feature reads do not hold the
    /// write lock. The small race window that remains is between the feature list
    /// capture and the PRD write-back, and it is accepted here, the same one the
    /// feature writes accept.
    pub async fn refresh_feature_index(&self) -> Vec<String> {
        let owned_prd = match DocPath::parse(PRD_PATH) {
            Ok(path) => path,
            Err(error) => {
                tracing::warn!(error = %error, "the PRD feature index cannot be refreshed");
                return vec![format!("the PRD feature index was not refreshed: {error}")];
            }
        };

        let writes = self.writes.clone();
        let recent_writes = self.recent_writes.clone();
        let root = self.root.clone();

        tokio::task::spawn_blocking(move || -> Vec<String> {
            // Copy the feature list before taking the write lock, so the directory
            // traversal and the feature reads do not hold it; the lock below guards
            // only the PRD read and write-back.
            let (entries, mut warnings) = tree::feature_entries(&root);

            let _guard = writes
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());

            let resolved = match std::fs::canonicalize(root.join(owned_prd.as_path())) {
                Ok(resolved) => resolved,
                Err(source) => {
                    let error = map_resolution_error(&owned_prd, source);
                    tracing::warn!(error = %error, "the PRD feature index cannot be resolved");
                    warnings.push(format!("the PRD feature index was not resolved: {error}"));
                    return warnings;
                }
            };
            let resolved = match ensure_inside_root(&root, &owned_prd, resolved) {
                Ok(resolved) => resolved,
                Err(error) => {
                    tracing::warn!(error = %error, "the PRD feature index cannot be resolved");
                    warnings.push(format!("the PRD feature index was not resolved: {error}"));
                    return warnings;
                }
            };
            let document = match read_document(&resolved, &owned_prd) {
                Ok(document) => document,
                Err(error) => {
                    tracing::warn!(error = %error, "the PRD feature index cannot be refreshed");
                    warnings.push(format!("the PRD feature index was not refreshed: {error}"));
                    return warnings;
                }
            };

            let updated = match rewrite(&document.content, &entries) {
                RewriteOutcome::Updated(updated) => updated,
                RewriteOutcome::NoMarkers => {
                    tracing::warn!(
                        "the PRD feature index cannot be refreshed: the markers are missing"
                    );
                    warnings.push(
                        "the PRD feature index was not refreshed: the feature index markers are missing"
                            .to_string(),
                    );
                    return warnings;
                }
            };

            if updated == document.content {
                // The rendered index already matches, so the PRD is left untouched.
                return warnings;
            }

            let expectation = Expectation::Existing(document.hash);
            let write_result = save_critical(
                &resolved,
                &owned_prd,
                &updated,
                &expectation,
                &recent_writes,
            );
            if let Err(error) = write_result {
                tracing::warn!(error = %error, "the PRD feature index could not be written");
                warnings.push(format!("the PRD feature index could not be written: {error}"));
                return warnings;
            }

            warnings
        })
        .await
        .map_err(|error| {
            // The refresh task panicked or was cancelled; report that failure as
            // a warning instead of hiding it behind a successful no-op refresh.
            let error = io::Error::other(error);
            tracing::warn!(error = %error, "the PRD feature index refresh task failed");
            StoreError::Io(error)
        })
        .unwrap_or_else(|error| vec![format!("the PRD feature index was not refreshed: {error}")])
    }

    /// Creates a document of `kind` from a built-in template (ticket 0015).
    ///
    /// The title is validated (trimmed, single line, at most
    /// [`templates::MAX_TITLE_LEN`] characters) and slugified, then rendered
    /// through the kind's template. The file is created with
    /// [`Expectation::Missing`] so an existing document is never overwritten, and
    /// numbered kinds allocate their next number. A ticket is created inside an
    /// existing epic's `tickets/` directory with an optional feature binding.
    ///
    /// The PRD feature index refresh after creating a feature is the caller's
    /// responsibility: callers that keep the index in sync invoke
    /// [`DocStore::refresh_feature_index`] themselves and surface its warnings.
    ///
    /// Only decision creation can produce warnings (a dangling supersedes
    /// pointer); the generic decision create answers with the default status and
    /// no supersedes pointer, so it cannot produce any and the decision-aware
    /// create owns the warning reporting instead of this generic path.
    pub async fn create(
        &self,
        kind: DocKind,
        title: &str,
        epic: Option<&str>,
        feature: Option<&str>,
    ) -> Result<DocPath, StoreError> {
        let trimmed = title.trim();
        if trimmed.is_empty()
            || trimmed.contains('\n')
            || trimmed.contains('\r')
            || trimmed.chars().count() > templates::MAX_TITLE_LEN
        {
            return Err(StoreError::InvalidTitle(trimmed.to_string()));
        }

        let title = trimmed.to_string();
        let slug = templates::slugify(&title)?;

        match kind {
            DocKind::Epic => {
                self.create_epic_at_next(slug.clone(), move |number| {
                    templates::epic_content(number, &title, &slug)
                })
                .await
            }
            DocKind::Feature => {
                let content = templates::feature_content(&title);
                let path = self.create_feature(&slug, &content).await?;
                Ok(path)
            }
            DocKind::Decision => {
                // Decisions carry the ADR metadata of ticket 0031; the generic
                // create serves the default `proposed` status of a new decision
                // and no supersedes pointer, so the default create can neither
                // dangle nor self-supersede and its create warnings are not
                // surfaced here; the decision-aware create returns them instead.
                let (created, _warnings) = self
                    .create_decision_at_next(
                        slug,
                        title,
                        crate::store::frontmatter::DecisionStatus::Proposed,
                        None,
                    )
                    .await?;
                // `_warnings` is always empty for this default create: it carries
                // no supersedes pointer, so nothing can dangle or self-supersede.
                Ok(created)
            }
            DocKind::Ticket => {
                let epic = epic.ok_or_else(|| {
                    StoreError::NotFound("no epic was given for a ticket".to_string())
                })?;
                self.ensure_ticket_epic(epic).await?;
                if let Some(feature) = feature {
                    self.ensure_ticket_feature(feature).await?;
                }
                let feature = feature.map(|feature| feature.to_string());
                self.create_ticket_at_next(epic.to_string(), slug.clone(), move |number| {
                    templates::ticket_content(number, &title, feature.as_deref())
                })
                .await
            }
            DocKind::Core(_) => Err(StoreError::NotAllowed(
                "a core document cannot be created".to_string(),
            )),
        }
    }

    /// Creates a new feature file at the slugified title, refusing to overwrite
    /// an existing feature through the missing-expectation write.
    async fn create_feature(&self, slug: &str, content: &str) -> Result<DocPath, StoreError> {
        let writes = self.writes.clone();
        let recent_writes = self.recent_writes.clone();
        let root = self.root.clone();
        let slug = slug.to_string();
        let content = content.to_string();

        tokio::task::spawn_blocking(move || -> Result<DocPath, StoreError> {
            let _guard = writes
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());

            let path = DocPath::parse(&format!("sdlc/features/{slug}.md"))?;
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

            crate::store::save_critical(
                &resolved,
                &path,
                &content,
                &Expectation::Missing,
                &recent_writes,
            )?;
            Ok(path)
        })
        .await
        .map_err(|err| StoreError::Io(std::io::Error::other(err)))?
    }

    /// A ticket can only be created inside an existing epic that has a
    /// description, so the target epic directory is validated before numbering.
    async fn ensure_ticket_epic(&self, epic: &str) -> Result<(), StoreError> {
        let raw = format!("sdlc/epics/{epic}/description.md");
        let path = self
            .root
            .join("sdlc")
            .join("epics")
            .join(epic)
            .join("description.md");
        let known = DocPath::parse(&raw)
            .map(|path| layout::kind(path.as_path()) == Some(DocKind::Epic))
            .unwrap_or(false)
            && tokio::fs::metadata(&path)
                .await
                .map(|metadata| metadata.is_file())
                .unwrap_or(false);
        if known {
            Ok(())
        } else {
            Err(StoreError::NotFound(format!(
                "no epic named {epic} has a description"
            )))
        }
    }

    /// A ticket's optional feature binding must name an existing feature.
    async fn ensure_ticket_feature(&self, feature: &str) -> Result<(), StoreError> {
        let raw = format!("sdlc/features/{feature}.md");
        let path = self
            .root
            .join("sdlc")
            .join("features")
            .join(format!("{feature}.md"));
        let known = DocPath::parse(&raw)
            .map(|path| layout::kind(path.as_path()) == Some(DocKind::Feature))
            .unwrap_or(false)
            && tokio::fs::metadata(&path)
                .await
                .map(|metadata| metadata.is_file())
                .unwrap_or(false);
        if known {
            Ok(())
        } else {
            Err(StoreError::NotFound(format!("no feature named {feature}")))
        }
    }

    /// Renames a document, keeping its kind and number (ticket 0016).
    ///
    /// Both paths are resolved before anything is touched on disk: the source
    /// must exist and the destination must resolve inside the root, so a symlink
    /// that escapes the project is rejected before the rename runs. The
    /// destination must not exist yet, so a collision fails with
    /// [`StoreError::AlreadyExists`] instead of overwriting.
    ///
    /// Epics rename their whole directory, so their `tickets/` move with them
    /// and no ticket front matter changes; the returned path is the new
    /// `description.md`. Tickets move only within their current epic.
    ///
    /// The check and the rename run inside one write critical section.
    ///
    /// The PRD feature index refresh after renaming a feature is the caller's
    /// responsibility: callers that keep the index in sync invoke
    /// [`DocStore::refresh_feature_index`] themselves and surface its warnings.
    pub async fn rename(&self, from: &DocPath, to: &DocPath) -> Result<DocPath, StoreError> {
        let source = self.resolve(from, ResolveMode::Existing).await?;
        let target = self.resolve_destination(to).await?;

        let writes = self.writes.clone();
        let recent_writes = self.recent_writes.clone();
        let owned_from = from.clone();
        let owned_to = to.clone();

        tokio::task::spawn_blocking(move || -> Result<DocPath, StoreError> {
            let _guard = writes
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            rename_resolved(&owned_from, &owned_to, &source, &target, &recent_writes)
        })
        .await
        .map_err(|err| StoreError::Io(io::Error::other(err)))?
    }

    /// Deletes a document, refusing core documents (ticket 0016).
    ///
    /// An epic is only deletable while its `tickets/` directory holds no `.md`
    /// file; otherwise the delete fails with [`StoreError::EpicHasTickets`] and
    /// nothing is removed. Deleting a feature orphans the tickets that point at
    /// it, which the report counts without deleting or rewriting them.
    ///
    /// The removal runs inside one write critical section.
    ///
    /// The PRD feature index refresh after deleting a feature is the caller's
    /// responsibility: callers that keep the index in sync invoke
    /// [`DocStore::refresh_feature_index`] themselves and surface its warnings.
    pub async fn delete(&self, path: &DocPath) -> Result<DeleteReport, StoreError> {
        let resolved = self.resolve(path, ResolveMode::Existing).await?;

        let writes = self.writes.clone();
        let recent_writes = self.recent_writes.clone();
        let root = self.root.clone();
        let owned = path.clone();

        tokio::task::spawn_blocking(move || -> Result<DeleteReport, StoreError> {
            let _guard = writes
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            delete_resolved(&root, &owned, &resolved, &recent_writes)
        })
        .await
        .map_err(|err| StoreError::Io(io::Error::other(err)))?
    }

    /// Resolves the destination of a rename that may not exist yet.
    ///
    /// File renames resolve with [`ResolveMode::NewFile`]; an epic rename
    /// resolves to the new epic directory, because the destination of an epic
    /// rename is a directory whose parent (the new epic directory) does not
    /// exist yet.
    async fn resolve_destination(&self, to: &DocPath) -> Result<PathBuf, StoreError> {
        let kind = layout::kind(to.as_path()).ok_or_else(|| not_allowed(to.as_path()))?;
        if let DocKind::Epic = kind {
            let epic = layout::epic_of(to.as_path())
                .ok_or_else(|| not_allowed(to.as_path()))?
                .to_string();
            let base = self.root.join("sdlc").join("epics");
            let resolved_base = tokio::fs::canonicalize(&base)
                .await
                .map_err(|err| map_resolution_error(to, err))?;
            let resolved_base = ensure_inside_root(&self.root, to, resolved_base)?;

            return Ok(resolved_base.join(epic));
        }

        self.resolve(to, ResolveMode::NewFile).await
    }
}

/// Renames an already-resolved source onto an already-resolved destination,
/// touching the filesystem only after every check passed.
///
/// `source` and `target` are the resolved paths of the unit being renamed: the
/// epic directory for an epic, the document file for feature, decision and
/// ticket renames.
fn rename_resolved(
    from: &DocPath,
    to: &DocPath,
    source: &Path,
    target: &Path,
    recent: &RecentWrites,
) -> Result<DocPath, StoreError> {
    let from_kind = layout::kind(from.as_path()).ok_or_else(|| not_allowed(from.as_path()))?;
    let to_kind = layout::kind(to.as_path()).ok_or_else(|| not_allowed(to.as_path()))?;
    if from_kind != to_kind {
        // A rename never crosses kinds; a core document is refused here as well,
        // because renaming a core document is not an allowed operation.
        return Err(StoreError::NotAllowed(format!(
            "cannot rename a document of kind {from_kind:?} onto a document of kind {to_kind:?}"
        )));
    }

    let from_name = layout::document_name(from.as_path())
        .unwrap_or_default()
        .to_string();
    let to_name = layout::document_name(to.as_path())
        .unwrap_or_default()
        .to_string();
    let from_number = layout::numbered_number(&from_name);
    let to_number = layout::numbered_number(&to_name);
    if let (Some(from_number), Some(to_number)) = (from_number, to_number)
        && from_number != to_number
    {
        return Err(StoreError::NumberChange { from_name, to_name });
    }

    match from_kind {
        DocKind::Core(_) => Err(not_allowed(from.as_path())),
        DocKind::Epic => {
            // `source` is the epic directory here because the resolved source of
            // an epic is its `description.md`, and the whole directory moves, so
            // its `tickets/` travel with it.
            let from_dir = source.parent().ok_or_else(|| not_allowed(source))?;
            if path_exists(target)? {
                return Err(StoreError::AlreadyExists(
                    to.as_path().display().to_string(),
                ));
            }
            let moved_hash = content_hash(&std::fs::read(source)?);
            std::fs::rename(from_dir, target)?;
            // The rename moves the content, so the destination is a write of the
            // same content and the source a delete; both are remembered, so the
            // watcher suppresses the events of the move itself (ticket 0055).
            recent.record_delete(&from.as_path().to_string_lossy());
            recent.record_write(&to.as_path().to_string_lossy(), &moved_hash);
            Ok(to.clone())
        }
        DocKind::Ticket => {
            // A ticket moves only within its current epic, so the epic
            // directory has to be the same before anything is renamed.
            let from_epic =
                layout::epic_of(from.as_path()).ok_or_else(|| not_allowed(from.as_path()))?;
            let to_epic = layout::epic_of(to.as_path()).ok_or_else(|| not_allowed(to.as_path()))?;
            if from_epic != to_epic {
                return Err(StoreError::NotAllowed(format!(
                    "cannot rename a ticket onto another epic: {from_epic} -> {to_epic}"
                )));
            }
            if path_exists(target)? {
                return Err(StoreError::AlreadyExists(
                    to.as_path().display().to_string(),
                ));
            }
            let moved_hash = content_hash(&std::fs::read(source)?);
            std::fs::rename(source, target)?;
            // The rename moves the content, so the destination is a write of the
            // same content and the source a delete; both are remembered, so the
            // watcher suppresses the events of the move itself (ticket 0055).
            recent.record_delete(&from.as_path().to_string_lossy());
            recent.record_write(&to.as_path().to_string_lossy(), &moved_hash);
            Ok(to.clone())
        }
        DocKind::Feature | DocKind::Decision => {
            if path_exists(target)? {
                return Err(StoreError::AlreadyExists(
                    to.as_path().display().to_string(),
                ));
            }
            let moved_hash = content_hash(&std::fs::read(source)?);
            std::fs::rename(source, target)?;
            // The rename moves the content, so the destination is a write of the
            // same content and the source a delete; both are remembered, so the
            // watcher suppresses the events of the move itself (ticket 0055).
            recent.record_delete(&from.as_path().to_string_lossy());
            recent.record_write(&to.as_path().to_string_lossy(), &moved_hash);
            Ok(to.clone())
        }
    }
}

/// Deletes an already-resolved document, refusing core documents (ticket 0016).
fn delete_resolved(
    root: &Path,
    path: &DocPath,
    resolved: &Path,
    recent: &RecentWrites,
) -> Result<DeleteReport, StoreError> {
    let kind = layout::kind(path.as_path()).ok_or_else(|| not_allowed(path.as_path()))?;

    match kind {
        DocKind::Core(_) => Err(not_allowed(path.as_path())),
        DocKind::Epic => {
            let epic_dir = resolved.parent().ok_or_else(|| not_allowed(resolved))?;
            let tickets_dir = epic_dir.join("tickets");
            let tickets = count_tickets(&tickets_dir);
            if tickets > 0 {
                return Err(StoreError::EpicHasTickets { tickets });
            }
            std::fs::remove_dir_all(epic_dir)?;
            recent.record_delete(&path.as_path().to_string_lossy());
            Ok(DeleteReport {
                orphaned_tickets: 0,
            })
        }
        DocKind::Feature => {
            let feature = layout::document_name(path.as_path()).unwrap_or_default();
            let orphaned = orphaned_tickets(root, feature);
            std::fs::remove_file(resolved)?;
            recent.record_delete(&path.as_path().to_string_lossy());
            Ok(DeleteReport {
                orphaned_tickets: orphaned,
            })
        }
        DocKind::Decision | DocKind::Ticket => {
            std::fs::remove_file(resolved)?;
            recent.record_delete(&path.as_path().to_string_lossy());
            Ok(DeleteReport {
                orphaned_tickets: 0,
            })
        }
    }
}

/// Counts the ticket files inside a tickets directory.
fn count_tickets(dir: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut count = 0;
    for entry in entries.filter_map(|entry| entry.ok()) {
        let file_name = entry.file_name();
        if !file_name.to_string_lossy().ends_with(".md") {
            continue;
        }
        if !entry
            .file_type()
            .map(|metadata| metadata.is_file())
            .unwrap_or(false)
        {
            continue;
        }
        count += 1;
    }
    count
}

/// Counts the tickets whose feature binding names a deleted feature.
///
/// The tickets themselves are left untouched: they become orphans, and the
/// caller reports the count instead of rewriting them.
fn orphaned_tickets(root: &Path, feature: &str) -> usize {
    // The deleted feature name may carry the `.md` extension; ticket feature
    // bindings may as well, so both are normalized to the bare slug.
    let feature = feature.strip_suffix(".md").unwrap_or(feature);
    let Ok(entries) = std::fs::read_dir(root.join("sdlc").join("epics")) else {
        return 0;
    };
    let mut orphaned = 0;
    for entry in entries.filter_map(|entry| entry.ok()) {
        let tickets_dir = entry.path().join("tickets");
        let Ok(tickets) = std::fs::read_dir(&tickets_dir) else {
            continue;
        };
        for ticket in tickets.filter_map(|ticket| ticket.ok()) {
            let ticket_file = ticket.file_name().to_string_lossy().into_owned();
            let content = match std::fs::read_to_string(ticket.path()) {
                Ok(content) => content,
                Err(err) => {
                    tracing::warn!(
                        file = %ticket.path().display(),
                        error = %err,
                        "skipping unreadable ticket during orphan scan"
                    );
                    continue;
                }
            };
            let meta = crate::store::frontmatter::parse_ticket(&content, &ticket_file);
            let bound = meta
                .feature
                .as_deref()
                .is_some_and(|bound| bound.strip_suffix(".md").unwrap_or(bound) == feature);
            if bound {
                orphaned += 1;
            }
        }
    }
    orphaned
}

/// The refusal of an epic that still holds tickets, singular for a single
/// ticket and plural otherwise, so the message never renders a bare "1 tickets"
/// (ticket 0030, final review round); the store display and the API error share
/// this one helper so their wording cannot drift apart.
pub(crate) fn epic_has_tickets_message(tickets: usize) -> String {
    if tickets == 1 {
        "epic has 1 ticket and cannot be deleted".to_string()
    } else {
        format!("epic has {tickets} tickets and cannot be deleted")
    }
}

/// Runs the atomic check-and-write sequence under the caller's write lock:
/// an `Existing` hash replaces the content only while the file on disk still
/// hashes to it, a `Missing` expectation creates the file while refusing to
/// overwrite anything already present. The bytes land in a synced temp file
/// that is renamed over the target, so a failure never leaves a partial
/// document behind.
fn save_critical(
    resolved: &Path,
    owned: &DocPath,
    owned_content: &str,
    owned_expectation: &Expectation,
    recent: &RecentWrites,
) -> Result<String, StoreError> {
    let new_hash = content_hash(owned_content.as_bytes());

    match owned_expectation {
        Expectation::Existing(hash) => {
            // Hash the raw bytes: comparing the decoded string would turn
            // externally rewritten non-UTF-8 content into an `InvalidUtf8`
            // instead of the documented `Conflict` carrying the disk hash.
            let current_bytes =
                std::fs::read(resolved).map_err(|source| map_read_error(owned, source))?;
            let current_hash = content_hash(&current_bytes);
            if current_hash != hash.as_str() {
                return Err(StoreError::Conflict { current_hash });
            }
            // The content is known before the write runs, so the hash is
            // recorded before the write completes and the watcher event cannot
            // arrive first (ticket 0055).
            recent.record_write(&owned.as_path().to_string_lossy(), &new_hash);
            write_atomic(resolved, owned_content.as_bytes())?;
        }
        Expectation::Missing => {
            if path_exists(resolved)? {
                return Err(StoreError::AlreadyExists(
                    owned.as_path().display().to_string(),
                ));
            }
            recent.record_write(&owned.as_path().to_string_lossy(), &new_hash);
            write_atomic(resolved, owned_content.as_bytes())?;
        }
    }

    Ok(new_hash)
}

/// Reads the file at an already-resolved path into a [`Document`].
///
/// A missing file surfaces as [`StoreError::NotFound`]; an invalid UTF-8
/// document surfaces as [`StoreError::InvalidUtf8`].
fn read_document(resolved: &Path, path: &DocPath) -> Result<Document, StoreError> {
    let file_bytes = std::fs::read(resolved).map_err(|source| map_read_error(path, source))?;
    let hash = content_hash(&file_bytes);
    let content = String::from_utf8(file_bytes)
        .map_err(|_| StoreError::InvalidUtf8(path.as_path().display().to_string()))?;

    Ok(Document {
        path: path.clone(),
        content,
        hash,
    })
}

/// Atomically replaces `target` with `bytes`.
///
/// The bytes land in a hidden temp file next to the target, which is synced
/// and then renamed over the target, so a reader never observes a partially
/// written file and a crash never leaves one behind. The temp file is
/// removed on every error path.
///
/// Renaming over an existing file is atomic on Unix; on Windows the rename
/// maps to a replace operation, and since the temp file always lives in the
/// same directory as the target no rename ever crosses a volume.
fn write_atomic(target: &Path, bytes: &[u8]) -> Result<(), StoreError> {
    let parent = target.parent().ok_or_else(|| not_allowed(target))?;
    let file_name = target.file_name().ok_or_else(|| not_allowed(target))?;
    let temp = temp_path(parent, file_name);

    let written = (|| -> Result<(), StoreError> {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;

        // The handle must be released before the rename: on Windows an open
        // source file blocks the replace operation.
        drop(file);

        // A rename keeps the temp file's mode, which is the platform default
        // for a newly created file. Copy the replaced file's mode first, so
        // overwriting never widens permissions beyond what the file had.
        if let Ok(metadata) = std::fs::metadata(target) {
            std::fs::set_permissions(&temp, metadata.permissions())?;
        }

        std::fs::rename(&temp, target)?;
        // A directory fsync that fails after a successful rename does not
        // invalidate the write; treat it as a durability warning, not an error.
        if let Err(err) = sync_parent_dir(parent) {
            tracing::warn!(error = %err, "failed to sync parent directory after rename");
        }

        Ok(())
    })();

    if written.is_err() {
        let _ = std::fs::remove_file(&temp);
    }

    written
}

/// Reports whether a path exists, translating `NotFound` into `false`.
fn path_exists(path: &Path) -> Result<bool, StoreError> {
    match std::fs::metadata(path) {
        Ok(_) => Ok(true),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(source) => Err(StoreError::Io(source)),
    }
}

/// Builds the temp file path used for an atomic write: a hidden file in the
/// same directory as the target, named `.<file_name>.tmp-<suffix>`.
fn temp_path(dir: &Path, file_name: &OsStr) -> PathBuf {
    dir.join(format!(
        ".{}.tmp-{}",
        file_name.to_string_lossy(),
        temp_suffix()
    ))
}

/// Temp file suffix combining a per-process sequence with the process id, so
/// writes from different processes cannot reuse a temp name.
fn temp_suffix() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let sequence = COUNTER.fetch_add(1, Ordering::Relaxed);

    format!("{}-{}", sequence, std::process::id())
}

/// Syncs the directory that received a rename so the new entry survives a crash.
///
/// A rename updates a directory entry, which is not covered by syncing the
/// file itself, so the parent directory is synced afterwards. Directory fsync
/// is not portable through `std` on every platform, so other platforms skip it
/// and rely on the atomic rename alone.
#[cfg(unix)]
fn sync_parent_dir(dir: &Path) -> Result<(), StoreError> {
    let file = std::fs::File::open(dir)?;
    file.sync_all()?;
    Ok(())
}

#[cfg(not(unix))]
fn sync_parent_dir(_dir: &Path) -> Result<(), StoreError> {
    Ok(())
}

/// Returns `resolved` only when it is the root or lives below it.
///
/// A free function rather than a [`DocStore`] method so the PRD feature-index
/// refresh can canonicalize and check the resolved path inside its own write
/// critical section, where a `&self` borrow cannot be captured by a `'static`
/// blocking closure.
fn ensure_inside_root(
    root: &Path,
    path: &DocPath,
    resolved: PathBuf,
) -> Result<PathBuf, StoreError> {
    if resolved.starts_with(root) {
        tracing::debug!(resolved = %resolved.display(), "resolved document path");
        Ok(resolved)
    } else {
        Err(not_allowed(path.as_path()))
    }
}

/// Builds the path-shaped error used for every rejected resolution.
fn not_allowed(path: &Path) -> StoreError {
    StoreError::NotAllowed(path.display().to_string())
}

/// Maps a canonicalization failure without leaking the absolute root path.
fn map_resolution_error(path: &DocPath, err: std::io::Error) -> StoreError {
    if err.kind() == std::io::ErrorKind::NotFound {
        StoreError::NotFound(path.as_path().display().to_string())
    } else {
        StoreError::Io(err)
    }
}

/// Maps a read failure, translating "missing" into [`StoreError::NotFound`].
fn map_read_error(path: &DocPath, source: std::io::Error) -> StoreError {
    if source.kind() == std::io::ErrorKind::NotFound {
        StoreError::NotFound(path.as_path().display().to_string())
    } else {
        StoreError::Io(source)
    }
}

/// Lowercase hex digits, indexed by the high and low nibble of a byte.
const HEX_DIGITS: &[u8; 16] = b"0123456789abcdef";

/// Computes the lowercase hex SHA-256 digest of `bytes`.
///
/// Used by reads for conflict detection and by tests for the known digests. The
/// raw bytes are hashed, never a decoded string, so the hash is stable across
/// BOMs and line-ending changes.
pub fn content_hash(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);

    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let value = usize::from(byte);
        hex.push(HEX_DIGITS[value / 16] as char);
        hex.push(HEX_DIGITS[value % 16] as char);
    }

    hex
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::layout::{CoreDoc, DocKind};
    use crate::store::path::DocPath;
    use std::io;

    fn prd_path() -> DocPath {
        DocPath::parse("sdlc/PRD.md").expect("sdlc/PRD.md is a valid document path")
    }

    async fn store_in_temp() -> (tempfile::TempDir, DocStore) {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = DocStore::new(dir.path().to_path_buf()).expect("store");

        (dir, store)
    }

    #[tokio::test]
    async fn hashes_known_content() {
        assert_eq!(
            content_hash(b"# PRD\n"),
            "939f54666f4869a7e1701b92eb14af3f8fad15c083e40bfd75de01996b30bdca"
        );
        assert_eq!(
            content_hash(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[tokio::test]
    async fn reads_content_and_returns_the_content_hash() {
        let (dir, store) = store_in_temp().await;
        std::fs::create_dir_all(dir.path().join("sdlc")).expect("sdlc dir");
        std::fs::write(dir.path().join("sdlc/PRD.md"), "# PRD\n").expect("write prd");

        let document = store.read(&prd_path()).await.expect("read prd");

        assert_eq!(document.content, "# PRD\n");
        assert_eq!(
            document.hash,
            "939f54666f4869a7e1701b92eb14af3f8fad15c083e40bfd75de01996b30bdca"
        );
        assert_eq!(document.path, prd_path());
    }

    #[tokio::test]
    async fn preserves_line_endings_and_trailing_whitespace() {
        let (dir, store) = store_in_temp().await;
        std::fs::create_dir_all(dir.path().join("sdlc")).expect("sdlc dir");
        let content = "# Title\r\n\r\nSome text   \n\n";
        std::fs::write(dir.path().join("sdlc/PRD.md"), content).expect("write prd");

        let document = store.read(&prd_path()).await.expect("read prd");

        assert_eq!(document.content, content);
    }

    #[tokio::test]
    async fn preserves_a_bom_as_content_bytes() {
        let (dir, store) = store_in_temp().await;
        std::fs::create_dir_all(dir.path().join("sdlc")).expect("sdlc dir");
        let content = "\u{feff}# PRD\nbody";
        std::fs::write(dir.path().join("sdlc/PRD.md"), content).expect("write prd");

        let document = store.read(&prd_path()).await.expect("read prd");

        assert_eq!(document.content, content);
    }

    #[tokio::test]
    async fn missing_document_returns_not_found() {
        let (dir, store) = store_in_temp().await;

        let error = store
            .read(&DocPath::parse("sdlc/features/nope.md").expect("valid path"))
            .await
            .expect_err("missing document");

        assert!(matches!(error, StoreError::NotFound(_)));
        assert!(dir.path().exists());
    }

    #[tokio::test]
    async fn invalid_utf8_returns_a_distinct_error() {
        let (dir, store) = store_in_temp().await;
        std::fs::create_dir_all(dir.path().join("sdlc")).expect("sdlc dir");
        std::fs::write(dir.path().join("sdlc/PRD.md"), [0xff, 0xfe, b'b', b'r'])
            .expect("write prd");

        let error = store
            .read(&prd_path())
            .await
            .expect_err("invalid utf8 document");

        assert!(matches!(error, StoreError::InvalidUtf8(_)));
    }

    #[tokio::test]
    async fn two_reads_return_the_same_hash() {
        let (dir, store) = store_in_temp().await;
        std::fs::create_dir_all(dir.path().join("sdlc")).expect("sdlc dir");
        std::fs::write(dir.path().join("sdlc/PRD.md"), "# PRD\n").expect("write prd");

        let first = store.read(&prd_path()).await.expect("read prd");
        let second = store.read(&prd_path()).await.expect("read prd");

        assert_eq!(first.hash, second.hash);
        assert_eq!(first.content, second.content);
    }

    #[test]
    fn store_error_mappings_are_distinct() {
        // The read mapping translates a missing file into `NotFound`.
        let error = map_read_error(
            &DocPath::parse("sdlc/PRD.md").expect("valid path"),
            io::Error::new(io::ErrorKind::NotFound, "missing"),
        );

        assert!(matches!(error, StoreError::NotFound(_)));

        let error = map_read_error(
            &DocPath::parse("sdlc/PRD.md").expect("valid path"),
            io::Error::other("spawn failed"),
        );

        assert!(matches!(error, StoreError::Io(_)));
    }

    #[test]
    fn document_exposes_resolved_path_kind() {
        let document = Document {
            path: prd_path(),
            content: "# PRD\n".to_string(),
            hash: content_hash(b"# PRD\n"),
        };

        assert_eq!(document.hash.len(), 64);
        assert_eq!(document.content, "# PRD\n");
    }

    #[test]
    fn store_error_and_document_types_are_part_of_the_api() {
        // Compiles-side assertion that the error enum and document are public.
        let error = StoreError::NotAllowed("sdlc/PRD.md".to_string());
        assert!(error.to_string().contains("not an allowed document path"));

        let doc = Document {
            path: prd_path(),
            content: "# PRD".to_string(),
            hash: content_hash(b"# PRD"),
        };
        assert_eq!(doc.hash, content_hash(b"# PRD"));
        let _ = CoreDoc::Prd;
        let _ = DocKind::Feature;
    }

    #[tokio::test]
    async fn write_with_the_current_hash_writes_and_returns_the_new_hash() {
        let (dir, store) = store_in_temp().await;
        std::fs::create_dir_all(dir.path().join("sdlc")).expect("sdlc dir");
        std::fs::write(dir.path().join("sdlc/PRD.md"), "# PRD\n").expect("write prd");
        let expectation = Expectation::Existing(content_hash(b"# PRD\n"));

        let new_hash = store
            .write(&prd_path(), "# Fresh\n", &expectation)
            .await
            .expect("the write should succeed");

        let document = store.read(&prd_path()).await.expect("read prd");
        assert_eq!(document.content, "# Fresh\n");
        assert_eq!(document.hash, new_hash);
        assert_eq!(new_hash, content_hash(b"# Fresh\n"));
    }

    #[tokio::test]
    async fn write_with_a_stale_hash_returns_conflict_and_keeps_the_content() {
        let (dir, store) = store_in_temp().await;
        std::fs::create_dir_all(dir.path().join("sdlc")).expect("sdlc dir");
        std::fs::write(dir.path().join("sdlc/PRD.md"), "# PRD\n").expect("write prd");
        let expectation = Expectation::Existing(content_hash(b""));

        let error = store
            .write(&prd_path(), "# Rewritten\n", &expectation)
            .await
            .expect_err("a stale hash must conflict");

        match error {
            StoreError::Conflict { current_hash } => {
                assert_eq!(current_hash, content_hash(b"# PRD\n"));
            }
            other => panic!("expected a conflict, got {other:?}"),
        }
        assert_eq!(
            std::fs::read_to_string(dir.path().join("sdlc/PRD.md")).expect("read back"),
            "# PRD\n"
        );
    }

    #[tokio::test]
    async fn missing_expectation_creates_the_file() {
        let (dir, store) = store_in_temp().await;
        std::fs::create_dir_all(dir.path().join("sdlc")).expect("sdlc dir");

        let new_hash = store
            .write(&prd_path(), "# Created\n", &Expectation::Missing)
            .await
            .expect("the create should succeed");

        let document = store.read(&prd_path()).await.expect("read prd");
        assert_eq!(document.content, "# Created\n");
        assert_eq!(new_hash, content_hash(b"# Created\n"));
        assert!(dir.path().join("sdlc/PRD.md").exists());
    }

    #[tokio::test]
    async fn missing_expectation_twice_returns_already_exists() {
        let (dir, store) = store_in_temp().await;
        std::fs::create_dir_all(dir.path().join("sdlc")).expect("sdlc dir");
        std::fs::write(dir.path().join("sdlc/PRD.md"), "# PRD\n").expect("write prd");

        let error = store
            .write(&prd_path(), "# Other\n", &Expectation::Missing)
            .await
            .expect_err("a second create must collide");

        assert!(
            matches!(error, StoreError::AlreadyExists(_)),
            "got {error:?}"
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("sdlc/PRD.md")).expect("read back"),
            "# PRD\n"
        );
    }

    #[tokio::test]
    async fn write_leaves_no_temp_file_behind_after_a_successful_write() {
        let (dir, store) = store_in_temp().await;
        std::fs::create_dir_all(dir.path().join("sdlc")).expect("sdlc dir");
        std::fs::write(dir.path().join("sdlc/PRD.md"), "# PRD\n").expect("write prd");
        let expectation = Expectation::Existing(content_hash(b"# PRD\n"));

        store
            .write(&prd_path(), "# Fresh\n", &expectation)
            .await
            .expect("the write should succeed");

        let leftovers: Vec<String> = std::fs::read_dir(dir.path().join("sdlc"))
            .expect("read the document directory")
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().to_string())
            .filter(|name| name.contains(".tmp-"))
            .collect();

        assert!(leftovers.is_empty(), "leftover temp files: {leftovers:?}");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn write_does_not_widen_file_permissions() {
        let (dir, store) = store_in_temp().await;
        std::fs::create_dir_all(dir.path().join("sdlc")).expect("sdlc dir");
        let target = dir.path().join("sdlc/PRD.md");
        std::fs::write(&target, "# PRD\n").expect("write prd");
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600))
            .expect("owner-only write");
        let expectation = Expectation::Existing(content_hash(b"# PRD\n"));

        store
            .write(&prd_path(), "# Fresh\n", &expectation)
            .await
            .expect("the write should succeed");

        let mode = std::fs::metadata(&target)
            .expect("file metadata after write")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600, "an overwrite must not widen file permissions");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn failed_write_removes_temp_files_and_keeps_the_original_content() {
        use std::os::unix::fs::PermissionsExt;

        let (dir, store) = store_in_temp().await;
        std::fs::create_dir_all(dir.path().join("sdlc")).expect("sdlc dir");
        let target = dir.path().join("sdlc/PRD.md");
        std::fs::write(&target, "# PRD\n").expect("write prd");
        let expectation = Expectation::Existing(content_hash(b"# PRD\n"));
        let document_dir = dir.path().join("sdlc");
        let permissions = std::fs::metadata(&document_dir)
            .expect("document dir")
            .permissions();
        std::fs::set_permissions(&document_dir, std::fs::Permissions::from_mode(0o500))
            .expect("lock the document directory");

        let error = store
            .write(&prd_path(), "# Fresh\n", &expectation)
            .await
            .expect_err("a read-only directory must fail the write");

        std::fs::set_permissions(&document_dir, permissions).expect("restore the directory");
        assert!(matches!(error, StoreError::Io(_)), "got {error:?}");
        assert_eq!(
            std::fs::read_to_string(&target).expect("read back"),
            "# PRD\n"
        );
        let leftovers: Vec<String> = std::fs::read_dir(&document_dir)
            .expect("read the document directory")
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().to_string())
            .filter(|name| name.contains(".tmp-"))
            .collect();
        assert!(leftovers.is_empty(), "leftover temp files: {leftovers:?}");
    }

    #[tokio::test]
    async fn writes_crlf_trailing_whitespace_and_empty_content_exactly() {
        let (dir, store) = store_in_temp().await;
        std::fs::create_dir_all(dir.path().join("sdlc")).expect("sdlc dir");
        let original = "# Old\n";
        std::fs::write(dir.path().join("sdlc/PRD.md"), original).expect("write prd");
        let expectation = Expectation::Existing(content_hash(original.as_bytes()));

        let hash = store
            .write(&prd_path(), "# Title\r\n\r\nSome text   \n\n", &expectation)
            .await
            .expect("the write should succeed");

        let document = store.read(&prd_path()).await.expect("read prd");
        assert_eq!(document.content, "# Title\r\n\r\nSome text   \n\n");
        assert_eq!(document.hash, hash);
        assert_eq!(
            document.hash,
            content_hash(b"# Title\r\n\r\nSome text   \n\n")
        );

        let expectation = Expectation::Existing(hash);
        let hash = store
            .write(&prd_path(), "", &expectation)
            .await
            .expect("the empty write should succeed");

        let document = store.read(&prd_path()).await.expect("read prd");
        assert_eq!(document.content, "");
        assert_eq!(document.hash, hash);
        assert_eq!(hash, content_hash(b""));
    }

    #[tokio::test]
    async fn two_writers_with_the_same_expected_hash_leave_exactly_one_winner() {
        let (dir, store) = store_in_temp().await;
        std::fs::create_dir_all(dir.path().join("sdlc")).expect("sdlc dir");
        std::fs::write(dir.path().join("sdlc/PRD.md"), "# PRD\n").expect("write prd");
        let path = prd_path();
        let expected = Expectation::Existing(content_hash(b"# PRD\n"));
        let (release, signaled) = tokio::sync::oneshot::channel::<()>();

        let task1_store = store.clone();
        let task1_path = path.clone();
        let task1_expectation = expected.clone();

        let first_task = tokio::spawn(async move {
            let result = task1_store
                .write(&task1_path, "# First\n", &task1_expectation)
                .await;
            let _ = release.send(());
            result
        });

        let task2_store = store.clone();
        let task2_path = path.clone();
        let task2_expectation = expected.clone();
        let second_task = tokio::spawn(async move {
            let _ = signaled.await;
            task2_store
                .write(&task2_path, "# Second\n", &task2_expectation)
                .await
        });

        let first = first_task
            .await
            .expect("first writer task")
            .expect("first writer should win");
        let second = second_task
            .await
            .expect("second writer task")
            .expect_err("the late writer with a stale hash should conflict");

        assert_eq!(first, content_hash(b"# First\n"));
        assert!(
            matches!(second, StoreError::Conflict { .. }),
            "got {second:?}"
        );
        let document = store.read(&prd_path()).await.expect("read prd");
        assert_eq!(document.content, "# First\n");
    }

    #[tokio::test]
    async fn create_epic_writes_the_template_with_the_allocated_number() {
        let (dir, store) = store_in_temp().await;

        let path = store
            .create(DocKind::Epic, "Document store", None, None)
            .await
            .expect("the epic should be created");

        assert_eq!(
            path.as_path().display().to_string(),
            "sdlc/epics/0001-document-store/description.md"
        );

        let content = std::fs::read_to_string(
            dir.path()
                .join("sdlc/epics/0001-document-store/description.md"),
        )
        .expect("read the epic back");

        assert!(content.starts_with("---\nstatus: planned\n---\n\n"));
        assert!(content.contains("# 0001: Document store"));
        assert!(content.ends_with("calling the epic done.\n"));
        assert!(
            dir.path()
                .join("sdlc/epics/0001-document-store/tickets")
                .is_dir()
        );
    }

    #[tokio::test]
    async fn create_feature_writes_a_planned_document() {
        let (dir, store) = store_in_temp().await;

        let path = store
            .create(DocKind::Feature, "Export", None, None)
            .await
            .expect("the feature should be created");

        assert_eq!(
            path.as_path().display().to_string(),
            "sdlc/features/export.md"
        );

        let content = std::fs::read_to_string(dir.path().join("sdlc/features/export.md"))
            .expect("read the feature back");

        assert_eq!(content, "---\nstatus: planned\n---\n\n# Export\n");
    }

    #[tokio::test]
    async fn create_decision_writes_a_proposed_document() {
        let (dir, store) = store_in_temp().await;

        let path = store
            .create(DocKind::Decision, "Use axum", None, None)
            .await
            .expect("the decision should be created");

        assert_eq!(
            path.as_path().display().to_string(),
            "sdlc/decisions/0001-use-axum.md"
        );

        let content = std::fs::read_to_string(dir.path().join("sdlc/decisions/0001-use-axum.md"))
            .expect("read the decision back");

        assert!(content.starts_with("---\nstatus: proposed\n---\n\n"));
        assert!(content.contains("# 0001: Use axum"));
    }

    #[tokio::test]
    async fn create_ticket_writes_it_inside_the_epic_with_the_feature() {
        let (dir, store) = store_in_temp().await;
        std::fs::create_dir_all(dir.path().join("sdlc/features")).expect("feature dir");
        std::fs::create_dir_all(dir.path().join("sdlc/epics/0001-foundation")).expect("epic dir");
        std::fs::create_dir_all(dir.path().join("sdlc/epics/0001-foundation/tickets"))
            .expect("epic tickets dir");
        std::fs::write(
            dir.path().join("sdlc/epics/0001-foundation/description.md"),
            "---\nstatus: planned\n---\n\n# 0001: Foundation\n",
        )
        .expect("write the epic description");
        std::fs::write(
            dir.path().join("sdlc/features/export.md"),
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

        assert_eq!(
            path.as_path().display().to_string(),
            "sdlc/epics/0001-foundation/tickets/0001-add-csv-export.md"
        );

        let content = std::fs::read_to_string(
            dir.path()
                .join("sdlc/epics/0001-foundation/tickets/0001-add-csv-export.md"),
        )
        .expect("read the ticket back");

        assert!(content.starts_with("---\nfeature: export\nstatus: todo\ndepends_on: []\n---\n\n"));
        assert!(content.contains("# 0001: Add CSV export"));
        assert!(!content.contains("epic:"));

        let document = store.read(&path).await.expect("read the ticket back");
        assert_eq!(document.hash, content_hash(content.as_bytes()));
    }

    #[tokio::test]
    async fn create_ticket_without_a_feature_has_no_feature_key() {
        let (dir, store) = store_in_temp().await;
        std::fs::create_dir_all(dir.path().join("sdlc/epics/0001-foundation")).expect("epic dir");
        std::fs::create_dir_all(dir.path().join("sdlc/epics/0001-foundation/tickets"))
            .expect("epic tickets dir");
        std::fs::write(
            dir.path().join("sdlc/epics/0001-foundation/description.md"),
            "---\nstatus: planned\n---\n\n# 0001: Foundation\n",
        )
        .expect("write the epic description");

        let path = store
            .create(DocKind::Ticket, "Add export", Some("0001-foundation"), None)
            .await
            .expect("the ticket should be created without a feature");

        assert_eq!(
            path.as_path().display().to_string(),
            "sdlc/epics/0001-foundation/tickets/0001-add-export.md"
        );

        let content = std::fs::read_to_string(
            dir.path()
                .join("sdlc/epics/0001-foundation/tickets/0001-add-export.md"),
        )
        .expect("read the ticket back");

        assert!(content.starts_with("---\nstatus: todo\ndepends_on: []\n---\n\n"));
        assert!(!content.contains("feature:"));
    }

    #[tokio::test]
    async fn create_ticket_without_an_epic_fails() {
        let (_dir, store) = store_in_temp().await;

        let error = store
            .create(DocKind::Ticket, "Add export", None, None)
            .await
            .expect_err("a ticket without an epic must fail");

        assert!(matches!(error, StoreError::NotFound(_)), "got {error:?}");
    }

    #[tokio::test]
    async fn create_ticket_with_an_unknown_epic_fails() {
        let (_dir, store) = store_in_temp().await;

        let error = store
            .create(
                DocKind::Ticket,
                "Add export",
                Some("9999-nope"),
                Some("export"),
            )
            .await
            .expect_err("an unknown epic must fail");

        assert!(matches!(error, StoreError::NotFound(_)), "got {error:?}");
    }

    #[tokio::test]
    async fn create_ticket_with_an_unknown_feature_fails() {
        let (dir, store) = store_in_temp().await;
        std::fs::create_dir_all(dir.path().join("sdlc/epics/0001-foundation/tickets"))
            .expect("epic tickets dir");
        std::fs::write(
            dir.path().join("sdlc/epics/0001-foundation/description.md"),
            "---\nstatus: planned\n---\n\n# 0001: Foundation\n",
        )
        .expect("write the epic description");

        let error = store
            .create(
                DocKind::Ticket,
                "Add export",
                Some("0001-foundation"),
                Some("nope"),
            )
            .await
            .expect_err("an unknown feature must fail");

        assert!(matches!(error, StoreError::NotFound(_)), "got {error:?}");
    }

    #[tokio::test]
    async fn create_rejects_a_blank_title() {
        let (_dir, store) = store_in_temp().await;

        let error = store
            .create(DocKind::Feature, "   ", None, None)
            .await
            .expect_err("a blank title must be rejected");

        assert!(
            matches!(error, StoreError::InvalidTitle(_)),
            "got {error:?}"
        );
    }

    #[tokio::test]
    async fn create_rejects_a_multiline_title() {
        let (_dir, store) = store_in_temp().await;

        let error = store
            .create(DocKind::Feature, "Add export\n\nMore", None, None)
            .await
            .expect_err("a multi-line title must be rejected");

        assert!(
            matches!(error, StoreError::InvalidTitle(_)),
            "got {error:?}"
        );
    }

    #[tokio::test]
    async fn create_rejects_a_title_longer_than_two_hundred_chars() {
        let (_dir, store) = store_in_temp().await;
        let long_title = "a".repeat(201);

        let error = store
            .create(DocKind::Feature, &long_title, None, None)
            .await
            .expect_err("an over-long title must be rejected");

        assert!(
            matches!(error, StoreError::InvalidTitle(_)),
            "got {error:?}"
        );
    }

    #[tokio::test]
    async fn create_rejects_a_punctuation_only_title() {
        let (_dir, store) = store_in_temp().await;

        let error = store
            .create(DocKind::Feature, "!!!", None, None)
            .await
            .expect_err("a punctuation-only title must be rejected");

        assert!(
            matches!(error, StoreError::InvalidTitle(_)),
            "got {error:?}"
        );
    }

    #[tokio::test]
    async fn create_feature_with_a_duplicate_slug_fails_without_touching_the_file() {
        let (dir, store) = store_in_temp().await;

        store
            .create(DocKind::Feature, "Export", None, None)
            .await
            .expect("the first feature should be created");

        let error = store
            .create(DocKind::Feature, "export", None, None)
            .await
            .expect_err("a duplicate feature slug must fail");

        assert!(
            matches!(error, StoreError::AlreadyExists(_)),
            "got {error:?}"
        );

        let content = std::fs::read_to_string(dir.path().join("sdlc/features/export.md"))
            .expect("read the feature back");
        assert_eq!(content, "---\nstatus: planned\n---\n\n# Export\n");
    }

    // ------------------------------------------------------------------
    // Ticket 0029: PRD feature index refresh

    #[cfg(unix)]
    #[tokio::test]
    async fn refresh_feature_index_returns_a_warning_when_the_prd_write_fails() {
        use std::os::unix::fs::PermissionsExt;

        let (dir, store) = store_in_temp().await;
        std::fs::create_dir_all(dir.path().join("sdlc/features")).expect("features dir");
        std::fs::write(
            dir.path().join("sdlc/features/export.md"),
            "---\nstatus: planned\n---\n\n# Export\n",
        )
        .expect("write the feature");
        let prd = format!(
            "# Product Requirements Document\n\n## Purpose\n\nTODO: write the purpose.\n\n{}\n{}\n",
            crate::store::index::FEATURE_INDEX_START,
            crate::store::index::FEATURE_INDEX_END,
        );
        std::fs::write(dir.path().join("sdlc/PRD.md"), prd).expect("write the prd");
        let document_dir = dir.path().join("sdlc");
        let permissions = std::fs::metadata(&document_dir)
            .expect("read the document dir")
            .permissions();
        std::fs::set_permissions(&document_dir, std::fs::Permissions::from_mode(0o500))
            .expect("lock the document directory");

        let warnings = store.refresh_feature_index().await;

        std::fs::set_permissions(&document_dir, permissions).expect("restore the directory");
        assert_eq!(warnings.len(), 1, "the refresh should warn, not fail");
        assert!(
            warnings[0].starts_with("the PRD feature index could not be written: "),
            "the warning should carry the write error, got: {}",
            warnings[0]
        );
    }

    #[tokio::test]
    async fn refresh_after_a_rename_captures_the_renamed_feature() {
        let (dir, store) = store_in_temp().await;
        std::fs::create_dir_all(dir.path().join("sdlc/features")).expect("features dir");
        std::fs::write(
            dir.path().join("sdlc/features/export.md"),
            "---\nstatus: planned\n---\n\n# Export\n",
        )
        .expect("write the feature");
        let prd = format!(
            "# Product Requirements Document\n\n## Purpose\n\nTODO: write the purpose.\n\n{}\n{}\n",
            crate::store::index::FEATURE_INDEX_START,
            crate::store::index::FEATURE_INDEX_END,
        );
        std::fs::write(dir.path().join("sdlc/PRD.md"), prd).expect("write the prd");

        let renamed = store
            .rename(
                &DocPath::parse("sdlc/features/export.md").expect("valid feature"),
                &DocPath::parse("sdlc/features/csv-export.md").expect("valid feature"),
            )
            .await
            .expect("the feature should be renamed");
        assert_eq!(
            renamed.as_path().display().to_string(),
            "sdlc/features/csv-export.md"
        );

        let warnings = store.refresh_feature_index().await;
        assert!(
            warnings.is_empty(),
            "the refresh should not warn: {warnings:?}"
        );

        let prd = store.read(&prd_path()).await.expect("read the prd");
        assert_eq!(
            prd.content,
            "# Product Requirements Document\n\n## Purpose\n\nTODO: write the purpose.\n\n<!-- sdlc:feature-index:start -->\n- [Export](features/csv-export.md) — planned\n<!-- sdlc:feature-index:end -->\n"
        );
    }

    #[tokio::test]
    async fn refresh_returns_a_warning_when_a_feature_is_skipped() {
        let (dir, store) = store_in_temp().await;
        std::fs::create_dir_all(dir.path().join("sdlc/features")).expect("features dir");
        std::fs::write(dir.path().join("sdlc/features/Bad.md"), "# Bad\n")
            .expect("write the bad feature");
        std::fs::write(
            dir.path().join("sdlc/features/export.md"),
            "---\nstatus: planned\n---\n\n# Export\n",
        )
        .expect("write the feature");
        let prd = format!(
            "# Product Requirements Document\n\n## Purpose\n\nTODO: write the purpose.\n\n{}\n{}\n",
            crate::store::index::FEATURE_INDEX_START,
            crate::store::index::FEATURE_INDEX_END,
        );
        std::fs::write(dir.path().join("sdlc/PRD.md"), prd).expect("write the prd");

        let warnings = store.refresh_feature_index().await;

        assert_eq!(
            warnings.len(),
            1,
            "the skipped feature should surface a warning: {warnings:?}"
        );
        assert!(
            warnings[0].contains("Bad.md"),
            "the warning should name the skipped path: {}",
            warnings[0]
        );

        let prd = store.read(&prd_path()).await.expect("read the prd");
        assert!(
            prd.content
                .contains("- [Export](features/export.md) — planned")
        );
        assert!(!prd.content.contains("Bad"));
    }

    #[tokio::test]
    async fn refresh_keeps_earlier_warnings_when_a_later_refresh_step_fails() {
        let (dir, store) = store_in_temp().await;
        std::fs::create_dir_all(dir.path().join("sdlc/features")).expect("features dir");
        std::fs::write(dir.path().join("sdlc/features/Bad.md"), "# Bad\n")
            .expect("write the bad feature");
        std::fs::write(
            dir.path().join("sdlc/features/export.md"),
            "---\nstatus: planned\n---\n\n# Export\n",
        )
        .expect("write the feature");
        // The PRD holds bytes that are not valid UTF-8, so the refresh cannot
        // read it and must still surface the warning collected for the skipped
        // feature instead of dropping it behind the fresh failure warning.
        std::fs::write(dir.path().join("sdlc/PRD.md"), [0xff, 0xfe, b'b', b'r'])
            .expect("write the prd");

        let warnings = store.refresh_feature_index().await;

        assert_eq!(
            warnings.len(),
            2,
            "the skipped feature and the failed PRD read should both surface: {warnings:?}"
        );
        assert!(
            warnings.iter().any(|warning| warning.contains("Bad.md")),
            "the skip warning should survive the later refresh failure: {warnings:?}"
        );
        assert!(
            warnings
                .iter()
                .any(|warning| warning.contains("the PRD feature index was not refreshed")),
            "the failed PRD read should be reported: {warnings:?}"
        );
    }

    // ------------------------------------------------------------------
    // Ticket 0016: rename and delete documents

    #[tokio::test]
    async fn rename_a_feature_keeps_the_content_and_returns_the_new_path() {
        let (dir, store) = store_in_temp().await;
        std::fs::create_dir_all(dir.path().join("sdlc/features")).expect("features dir");
        std::fs::write(
            dir.path().join("sdlc/features/export.md"),
            "---\nstatus: planned\n---\n\n# Export\n",
        )
        .expect("write the feature");

        let renamed = store
            .rename(
                &DocPath::parse("sdlc/features/export.md").expect("valid feature"),
                &DocPath::parse("sdlc/features/csv-export.md").expect("valid feature"),
            )
            .await
            .expect("the feature should be renamed");

        assert_eq!(
            renamed.as_path().display().to_string(),
            "sdlc/features/csv-export.md"
        );
        assert!(!dir.path().join("sdlc/features/export.md").exists());
        let document = store
            .read(&renamed)
            .await
            .expect("read the renamed feature");
        assert_eq!(document.content, "---\nstatus: planned\n---\n\n# Export\n");
    }

    #[tokio::test]
    async fn rename_a_ticket_moves_it_within_its_epic_and_keeps_the_number() {
        let (dir, store) = store_in_temp().await;
        std::fs::create_dir_all(dir.path().join("sdlc/epics/0001-foundation/tickets"))
            .expect("tickets dir");
        std::fs::write(
            dir.path().join("sdlc/epics/0001-foundation/description.md"),
            "---\nstatus: planned\n---\n\n# 0001: Foundation\n",
        )
        .expect("write the epic");
        std::fs::write(
            dir.path()
                .join("sdlc/epics/0001-foundation/tickets/0001-project.md"),
            "---\nstatus: todo\nfeature: export\n---\n\n# 0001: Project\n",
        )
        .expect("write the ticket");

        let renamed = store
            .rename(
                &DocPath::parse("sdlc/epics/0001-foundation/tickets/0001-project.md")
                    .expect("valid ticket"),
                &DocPath::parse("sdlc/epics/0001-foundation/tickets/0001-project-skeleton.md")
                    .expect("valid ticket"),
            )
            .await
            .expect("the ticket should be renamed");

        assert_eq!(
            renamed.as_path().display().to_string(),
            "sdlc/epics/0001-foundation/tickets/0001-project-skeleton.md"
        );
        assert!(
            !dir.path()
                .join("sdlc/epics/0001-foundation/tickets/0001-project.md")
                .exists()
        );
    }

    #[tokio::test]
    async fn rename_an_epic_moves_its_tickets_and_returns_the_new_description_path() {
        let (dir, store) = store_in_temp().await;
        std::fs::create_dir_all(dir.path().join("sdlc/epics/0001-foundation/tickets"))
            .expect("tickets dir");
        std::fs::write(
            dir.path().join("sdlc/epics/0001-foundation/description.md"),
            "---\nstatus: planned\n---\n\n# 0001: Foundation\n",
        )
        .expect("write the epic");
        std::fs::write(
            dir.path()
                .join("sdlc/epics/0001-foundation/tickets/0001-project.md"),
            "---\nstatus: todo\nfeature: export\n---\n\n# 0001: Project\n",
        )
        .expect("write the ticket");

        let renamed = store
            .rename(
                &DocPath::parse("sdlc/epics/0001-foundation/description.md").expect("valid epic"),
                &DocPath::parse("sdlc/epics/0001-platform/description.md").expect("valid epic"),
            )
            .await
            .expect("the epic should be renamed");

        assert_eq!(
            renamed.as_path().display().to_string(),
            "sdlc/epics/0001-platform/description.md"
        );
        assert!(!dir.path().join("sdlc/epics/0001-foundation").exists());

        let ticket = DocPath::parse("sdlc/epics/0001-platform/tickets/0001-project.md")
            .expect("valid ticket");
        let document = store.read(&ticket).await.expect("read the moved ticket");
        assert_eq!(
            document.content,
            "---\nstatus: todo\nfeature: export\n---\n\n# 0001: Project\n"
        );
    }

    #[tokio::test]
    async fn rename_rejects_a_changed_number() {
        let (dir, store) = store_in_temp().await;
        std::fs::create_dir_all(dir.path().join("sdlc/epics/0001-foundation")).expect("epic dir");
        std::fs::write(
            dir.path().join("sdlc/epics/0001-foundation/description.md"),
            "---\nstatus: planned\n---\n\n# 0001: Foundation\n",
        )
        .expect("write the epic");

        let error = store
            .rename(
                &DocPath::parse("sdlc/epics/0001-foundation/description.md").expect("valid epic"),
                &DocPath::parse("sdlc/epics/0002-platform/description.md").expect("valid epic"),
            )
            .await
            .expect_err("a rename must not change the document number");

        assert!(
            matches!(error, StoreError::NumberChange { .. }),
            "got {error:?}"
        );
        assert!(
            dir.path()
                .join("sdlc/epics/0001-foundation/description.md")
                .exists()
        );
    }

    #[tokio::test]
    async fn rename_rejects_cross_kind_renames() {
        let (dir, store) = store_in_temp().await;
        std::fs::create_dir_all(dir.path().join("sdlc/features")).expect("features dir");
        std::fs::create_dir_all(dir.path().join("sdlc/decisions")).expect("decisions dir");
        std::fs::write(
            dir.path().join("sdlc/features/export.md"),
            "---\nstatus: planned\n---\n\n# Export\n",
        )
        .expect("write the feature");

        let error = store
            .rename(
                &DocPath::parse("sdlc/features/export.md").expect("valid feature"),
                &DocPath::parse("sdlc/decisions/0001-use-axum.md").expect("valid decision"),
            )
            .await
            .expect_err("a rename may not cross kinds");

        assert!(matches!(error, StoreError::NotAllowed(_)), "got {error:?}");
        assert!(dir.path().join("sdlc/features/export.md").exists());
    }

    #[tokio::test]
    async fn rename_with_a_destination_collision_fails_without_overwriting() {
        let (dir, store) = store_in_temp().await;
        std::fs::create_dir_all(dir.path().join("sdlc/features")).expect("features dir");
        std::fs::write(
            dir.path().join("sdlc/features/export.md"),
            "---\nstatus: planned\n---\n\n# Export\n",
        )
        .expect("write the feature");
        std::fs::write(
            dir.path().join("sdlc/features/csv-export.md"),
            "---\nstatus: planned\n---\n\n# Csv export\n",
        )
        .expect("write the other feature");

        let error = store
            .rename(
                &DocPath::parse("sdlc/features/export.md").expect("valid feature"),
                &DocPath::parse("sdlc/features/csv-export.md").expect("valid feature"),
            )
            .await
            .expect_err("a destination collision must fail");

        assert!(
            matches!(error, StoreError::AlreadyExists(_)),
            "got {error:?}"
        );
        let content = std::fs::read_to_string(dir.path().join("sdlc/features/csv-export.md"))
            .expect("read the feature back");
        assert_eq!(content, "---\nstatus: planned\n---\n\n# Csv export\n");
    }

    #[tokio::test]
    async fn rename_a_missing_document_returns_not_found() {
        let (_dir, store) = store_in_temp().await;

        let error = store
            .rename(
                &DocPath::parse("sdlc/features/nope.md").expect("valid feature"),
                &DocPath::parse("sdlc/features/other.md").expect("valid feature"),
            )
            .await
            .expect_err("a rename of a missing document must fail");

        assert!(matches!(error, StoreError::NotFound(_)), "got {error:?}");
    }

    #[tokio::test]
    async fn rename_a_core_document_is_not_allowed() {
        let (dir, store) = store_in_temp().await;
        std::fs::create_dir_all(dir.path().join("sdlc")).expect("sdlc dir");
        std::fs::write(dir.path().join("sdlc/PRD.md"), "# PRD\n").expect("write prd");

        let error = store
            .rename(
                &DocPath::parse("sdlc/PRD.md").expect("valid core document"),
                &DocPath::parse("sdlc/architecture.md").expect("valid core document"),
            )
            .await
            .expect_err("a core document cannot be renamed");

        assert!(matches!(error, StoreError::NotAllowed(_)), "got {error:?}");
    }

    #[test]
    fn rename_a_document_name_that_is_only_casing_different_is_rejected() {
        // Valid document names are lowercase kebab-case, so a case-only rename
        // such as `export.md` -> `Export.md` cannot name a valid destination. It
        // is rejected while parsing, before the filesystem is touched; on a
        // case-insensitive filesystem an `fs::rename` would instead perform a
        // case-only rename, which no valid document name can express.
        assert!(
            matches!(
                DocPath::parse("sdlc/features/Export.md"),
                Err(StoreError::NotAllowed(_))
            ),
            "a case-variant document name must be rejected"
        );
    }

    #[tokio::test]
    async fn delete_a_core_document_is_not_allowed() {
        let (dir, store) = store_in_temp().await;
        std::fs::create_dir_all(dir.path().join("sdlc")).expect("sdlc dir");
        std::fs::write(dir.path().join("sdlc/PRD.md"), "# PRD\n").expect("write prd");

        let error = store
            .delete(&DocPath::parse("sdlc/PRD.md").expect("valid core document"))
            .await
            .expect_err("a core document cannot be deleted");

        assert!(matches!(error, StoreError::NotAllowed(_)), "got {error:?}");
        assert!(dir.path().join("sdlc/PRD.md").exists());
    }

    #[tokio::test]
    async fn delete_a_missing_document_returns_not_found() {
        let (_dir, store) = store_in_temp().await;

        let error = store
            .delete(&DocPath::parse("sdlc/features/nope.md").expect("valid feature"))
            .await
            .expect_err("a delete of a missing document must fail");

        assert!(matches!(error, StoreError::NotFound(_)), "got {error:?}");
    }

    #[tokio::test]
    async fn delete_each_core_document_is_not_allowed() {
        let (dir, store) = store_in_temp().await;
        std::fs::create_dir_all(dir.path().join("sdlc")).expect("sdlc dir");
        for (raw, content) in [
            ("sdlc/PRD.md", "# PRD\n"),
            ("sdlc/architecture.md", "# Architecture\n"),
            ("sdlc/conventions.md", "# Conventions\n"),
            ("AGENTS.md", "# sdlc\n"),
        ] {
            std::fs::write(dir.path().join(raw), content).expect("write the core document");
            let error = store
                .delete(&DocPath::parse(raw).expect("valid core document"))
                .await
                .expect_err("a core document cannot be deleted");

            assert!(matches!(error, StoreError::NotAllowed(_)), "got {error:?}");
        }
    }

    #[tokio::test]
    async fn delete_a_non_empty_epic_returns_conflict_and_deletes_nothing() {
        let (dir, store) = store_in_temp().await;
        std::fs::create_dir_all(dir.path().join("sdlc/epics/0001-foundation/tickets"))
            .expect("tickets dir");
        std::fs::write(
            dir.path().join("sdlc/epics/0001-foundation/description.md"),
            "---\nstatus: planned\n---\n\n# 0001: Foundation\n",
        )
        .expect("write the epic");
        std::fs::write(
            dir.path()
                .join("sdlc/epics/0001-foundation/tickets/0001-project.md"),
            "---\nstatus: todo\nfeature: export\n---\n\n# 0001: Project\n",
        )
        .expect("write the ticket");

        let error = store
            .delete(
                &DocPath::parse("sdlc/epics/0001-foundation/description.md").expect("valid epic"),
            )
            .await
            .expect_err("a non-empty epic must not be deletable");

        assert!(
            matches!(error, StoreError::EpicHasTickets { tickets: 1 }),
            "got {error:?}"
        );
        assert!(dir.path().join("sdlc/epics/0001-foundation").exists());
        assert!(
            dir.path()
                .join("sdlc/epics/0001-foundation/tickets/0001-project.md")
                .exists()
        );
    }

    #[tokio::test]
    async fn delete_an_empty_epic_removes_the_directory() {
        let (dir, store) = store_in_temp().await;
        std::fs::create_dir_all(dir.path().join("sdlc/epics/0001-foundation/tickets"))
            .expect("tickets dir");
        std::fs::write(
            dir.path().join("sdlc/epics/0001-foundation/description.md"),
            "---\nstatus: planned\n---\n\n# 0001: Foundation\n",
        )
        .expect("write the epic");

        let report = store
            .delete(
                &DocPath::parse("sdlc/epics/0001-foundation/description.md").expect("valid epic"),
            )
            .await
            .expect("the empty epic should be deletable");

        assert_eq!(report.orphaned_tickets, 0);
        assert!(!dir.path().join("sdlc/epics/0001-foundation").exists());
    }

    #[tokio::test]
    async fn delete_a_feature_reports_the_orphaned_tickets_without_deleting_them() {
        let (dir, store) = store_in_temp().await;
        std::fs::create_dir_all(dir.path().join("sdlc/epics/0001-foundation/tickets"))
            .expect("tickets dir");
        std::fs::create_dir_all(dir.path().join("sdlc/features")).expect("features dir");
        std::fs::write(
            dir.path().join("sdlc/epics/0001-foundation/description.md"),
            "---\nstatus: planned\n---\n\n# 0001: Foundation\n",
        )
        .expect("write the epic");
        std::fs::write(
            dir.path()
                .join("sdlc/epics/0001-foundation/tickets/0001-project.md"),
            "---\nstatus: todo\nfeature: export\n---\n\n# 0001: Project\n",
        )
        .expect("write the ticket");
        std::fs::write(
            dir.path().join("sdlc/features/export.md"),
            "---\nstatus: planned\n---\n\n# Export\n",
        )
        .expect("write the feature");

        let report = store
            .delete(&DocPath::parse("sdlc/features/export.md").expect("valid feature"))
            .await
            .expect("the feature should be deletable");

        assert_eq!(report.orphaned_tickets, 1);
        assert!(
            !dir.path().join("sdlc/features/export.md").exists(),
            "the feature itself should be deleted"
        );
        assert!(
            dir.path()
                .join("sdlc/epics/0001-foundation/tickets/0001-project.md")
                .exists(),
            "the orphaned ticket must not be deleted"
        );
    }

    #[tokio::test]
    async fn delete_a_decision_or_ticket_removes_the_file() {
        let (dir, store) = store_in_temp().await;
        std::fs::create_dir_all(dir.path().join("sdlc/decisions")).expect("decisions dir");
        std::fs::write(
            dir.path().join("sdlc/decisions/0001-use-axum.md"),
            "---\nstatus: proposed\n---\n\n# 0001: Use axum\n",
        )
        .expect("write the decision");

        let report = store
            .delete(&DocPath::parse("sdlc/decisions/0001-use-axum.md").expect("valid decision"))
            .await
            .expect("the decision should be deletable");

        assert_eq!(report.orphaned_tickets, 0);
        assert!(!dir.path().join("sdlc/decisions/0001-use-axum.md").exists());
    }
    #[test]
    fn recent_writes_suppress_their_own_write_and_publish_a_changed_hash() {
        let recent = RecentWrites::new();

        assert!(
            recent.should_publish("sdlc/PRD.md", None),
            "without a recorded write every change is published"
        );

        recent.record_write("sdlc/PRD.md", &content_hash(b"# Fresh\n"));
        assert!(
            !recent.should_publish("sdlc/PRD.md", Some(content_hash(b"# Fresh\n").as_str())),
            "the write the store performed is suppressed"
        );
        assert!(
            recent.should_publish("sdlc/PRD.md", Some(content_hash(b"# External\n").as_str())),
            "an external change of the same path is published"
        );
    }

    #[test]
    fn a_recorded_write_of_the_same_hash_twice_is_still_suppressed_once() {
        let recent = RecentWrites::new();
        let hash = content_hash(b"# Fresh\n");

        recent.record_write("sdlc/PRD.md", &hash);
        recent.record_write("sdlc/PRD.md", &hash);

        assert!(
            !recent.should_publish("sdlc/PRD.md", Some(hash.as_str())),
            "the repeated write of the same content is still sdlc's own write"
        );
    }

    #[test]
    fn a_recorded_delete_is_suppressed_while_the_document_is_missing() {
        let recent = RecentWrites::new();

        recent.record_delete("sdlc/PRD.md");
        assert!(
            !recent.should_publish("sdlc/PRD.md", None),
            "the delete the store performed is suppressed"
        );

        recent.record_write("sdlc/PRD.md", &content_hash(b"# Fresh\n"));
        assert!(
            !recent.should_publish("sdlc/PRD.md", Some(content_hash(b"# Fresh\n").as_str())),
            "a write recorded after the delete is published"
        );
    }

    #[test]
    fn recent_writes_expire_after_the_grace_period() {
        let recent = RecentWrites::with_limits(Duration::from_millis(20), MAX_RECENT_WRITES);
        let hash = content_hash(b"# Fresh\n");

        recent.record_write("sdlc/PRD.md", &hash);
        assert!(
            !recent.should_publish("sdlc/PRD.md", Some(hash.as_str())),
            "the recorded write is still comparable before it expires"
        );

        std::thread::sleep(Duration::from_millis(40));
        assert!(
            recent.should_publish("sdlc/PRD.md", Some(hash.as_str())),
            "an expired recorded write must not suppress an external change"
        );
    }

    #[test]
    fn recent_writes_prune_expired_entries_and_stay_bounded() {
        let recent = RecentWrites::with_limits(Duration::from_millis(20), 2);

        recent.record_write("sdlc/a.md", "hash-a");
        recent.record_write("sdlc/b.md", "hash-b");
        std::thread::sleep(Duration::from_millis(40));
        recent.record_write("sdlc/c.md", "hash-c");

        assert_eq!(
            recent.len(),
            1,
            "expired entries are pruned on insert and the map stays bounded"
        );
        assert!(
            recent.should_publish("sdlc/a.md", Some("hash-a")),
            "a pruned path is published again"
        );
    }

    #[tokio::test]
    async fn the_store_records_writes_so_the_watcher_can_suppress_them() {
        let (dir, store) = store_in_temp().await;
        std::fs::create_dir_all(dir.path().join("sdlc")).expect("sdlc dir");
        std::fs::write(dir.path().join("sdlc/PRD.md"), "# PRD\n").expect("write prd");
        let expectation = Expectation::Existing(content_hash(b"# PRD\n"));

        let new_hash = store
            .write(&prd_path(), "# Fresh\n", &expectation)
            .await
            .expect("the write should succeed");

        let recent = store.recent_writes();
        assert!(
            !recent.should_publish("sdlc/PRD.md", Some(new_hash.as_str())),
            "the save the store just performed is its own write and is suppressed"
        );
        assert!(
            recent.should_publish("sdlc/PRD.md", Some(content_hash(b"# External\n").as_str())),
            "an external change of the saved document is still published"
        );
    }

    #[tokio::test]
    async fn the_store_records_a_delete_so_the_watcher_can_suppress_it() {
        let (dir, store) = store_in_temp().await;
        std::fs::create_dir_all(dir.path().join("sdlc/features")).expect("features dir");
        std::fs::write(
            dir.path().join("sdlc/features/export.md"),
            "---\nstatus: planned\n---\n\n# Export\n",
        )
        .expect("write the feature");
        let path = DocPath::parse("sdlc/features/export.md").expect("valid path");
        let expectation =
            Expectation::Existing(content_hash(b"---\nstatus: planned\n---\n\n# Export\n"));

        store
            .write(&path, "# Fresh\n", &expectation)
            .await
            .expect("the write should succeed");
        store
            .delete(&path)
            .await
            .expect("the delete should succeed");

        let recent = store.recent_writes();
        assert!(
            !recent.should_publish("sdlc/features/export.md", None),
            "the delete the store just performed is its own write and is suppressed"
        );
    }
}
