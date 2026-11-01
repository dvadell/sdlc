//! The document store (ticket 0008).
//!
//! The store is the only component that touches project files. This ticket
//! establishes the validated [`DocPath`] and the layout whitelist; later
//! tickets add resolution, reads, writes and the `DocStore` itself.

pub mod layout;
pub mod path;

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
