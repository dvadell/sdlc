//! The document store (tickets 0008–0009).
//!
//! The store is the only component that touches project files. It owns the
//! validated [`DocPath`] and layout whitelist, and resolves those paths inside
//! a canonical project root so symlinks cannot escape it. Later tickets add
//! reads, writes and the rest of the store API.

pub mod layout;
pub mod path;

use std::path::PathBuf;

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
    /// the parent, because the file itself does not exist yet, and then joins
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
