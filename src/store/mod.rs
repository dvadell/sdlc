//! The document store (tickets 0008–0011).
//!
//! The store is the only component that touches project files. This layer
//! establishes the validated [`DocPath`], the layout whitelist, symlink-safe
//! resolution, content reads with a SHA-256 hash and atomic, hash-checked
//! writes; later tickets add creation, listing and scaffolding.

pub mod layout;
pub mod path;

use std::ffi::OsStr;
use std::io;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use sha2::{Digest, Sha256};
use thiserror::Error;

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

    /// A filesystem operation failed.
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub use layout::{CoreDoc, DocKind};
pub use path::DocPath;

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
        })
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
                self.ensure_inside_root(path, resolved)
            }
            ResolveMode::NewFile => {
                let relative = path.as_path();
                let parent = relative.parent().ok_or_else(|| not_allowed(relative))?;
                let file_name = relative.file_name().ok_or_else(|| not_allowed(relative))?;
                let joined_parent = self.root.join(parent);
                let resolved_parent = tokio::fs::canonicalize(&joined_parent)
                    .await
                    .map_err(|err| map_resolution_error(path, err))?;
                let resolved_parent = self.ensure_inside_root(path, resolved_parent)?;

                Ok(resolved_parent.join(file_name))
            }
        }
    }

    /// Returns `resolved` only when it is the root or lives below it.
    fn ensure_inside_root(&self, path: &DocPath, resolved: PathBuf) -> Result<PathBuf, StoreError> {
        if resolved.starts_with(&self.root) {
            tracing::debug!(resolved = %resolved.display(), "resolved document path");
            Ok(resolved)
        } else {
            Err(not_allowed(path.as_path()))
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

        let writes = self.writes.clone();

        tokio::task::spawn_blocking(move || -> Result<String, StoreError> {
            // The expectation check and the write are one critical section, so
            // two concurrent writers cannot both pass a check and rename. The
            // guard is only held inside the blocking task, never across an
            // `.await`.
            let _guard = writes
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());

            let new_hash = content_hash(owned_content.as_bytes());

            match owned_expectation {
                Expectation::Existing(hash) => {
                    // Hash the raw bytes: comparing the decoded string would turn
                    // externally rewritten non-UTF-8 content into an `InvalidUtf8`
                    // instead of the documented `Conflict` carrying the disk hash.
                    let current_bytes = std::fs::read(&resolved)
                        .map_err(|source| map_read_error(&owned, source))?;
                    let current_hash = content_hash(&current_bytes);
                    if current_hash != hash.as_str() {
                        return Err(StoreError::Conflict { current_hash });
                    }
                    write_atomic(&resolved, owned_content.as_bytes())?;
                }
                Expectation::Missing => {
                    if path_exists(&resolved)? {
                        return Err(StoreError::AlreadyExists(
                            owned.as_path().display().to_string(),
                        ));
                    }
                    write_atomic(&resolved, owned_content.as_bytes())?;
                }
            }

            Ok(new_hash)
        })
        .await
        .map_err(|err| StoreError::Io(io::Error::other(err)))?
    }
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
}
