//! HTTP route modules.

pub mod assets;
pub mod config;
pub mod docs;

use axum::{Router, extract::DefaultBodyLimit, routing::get};

use crate::state::AppState;

/// Maximum accepted save body: 2 MiB, enough for these documents and small
/// enough to fail oversized payloads early.
const MAX_SAVE_BODY_BYTES: usize = 2 * 1024 * 1024;

/// Builds the `/api` router.
///
/// The caller adds the asset fallback and the guard layer around this router.
pub fn router() -> Router<AppState> {
    let docs_route = get(docs::get_doc)
        .put(docs::put_doc)
        .layer(DefaultBodyLimit::max(MAX_SAVE_BODY_BYTES));

    Router::new()
        .route("/api/config", get(config::get_config))
        .route("/api/tree", get(docs::get_tree))
        .route("/api/docs/{*path}", docs_route)
}
