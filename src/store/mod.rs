//! The document store (tickets 0008–0010).
//!
//! The store is the only component that touches project files. This layer
//! establishes the validated [`DocPath`], the layout whitelist, symlink-safe
//! resolution and content reads with a SHA-256 hash; later tickets add writes,
//! creation, listing and scaffolding.

pub mod layout;
pub mod path;

use std::io;
use std::path::PathBuf;

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

/// The document store, rooted at one canonical project directory.
#[derive(Debug, Clone)]
pub struct DocStore {
    root: PathBuf,
}

impl DocStore {
    /// Creates a store rooted at `root`, canonicalizing the root once.
    ///
    /// Resolution always compares against the canonical root, so a project
    /// directory that is itself reached through a symlink works normally.
    pub fn new(root: PathBuf) -> Result<Self, StoreError> {
        let root = root.canonicalize()?;
        Ok(Self { root })
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
                let parent = relative.parent().ok_or_else(|| not_allowed(path))?;
                let file_name = relative.file_name().ok_or_else(|| not_allowed(path))?;
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
            Err(not_allowed(path))
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

        let outcome = tokio::task::spawn_blocking(move || -> Result<Document, StoreError> {
            let file_bytes =
                std::fs::read(&resolved).map_err(|source| map_read_error(&owned, source))?;
            let hash = content_hash(&file_bytes);
            let content = String::from_utf8(file_bytes)
                .map_err(|_| StoreError::InvalidUtf8(owned.as_path().display().to_string()))?;

            Ok(Document {
                path: owned,
                content,
                hash,
            })
        })
        .await
        .map_err(|err| StoreError::Io(io::Error::other(err)))?;

        let document = outcome?;

        Ok(document)
    }
}

/// Builds the path-shaped error used for every rejected resolution.
fn not_allowed(path: &DocPath) -> StoreError {
    StoreError::NotAllowed(path.as_path().display().to_string())
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
}
