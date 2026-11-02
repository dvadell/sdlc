//! HTTP route modules.

pub mod assets;
pub mod config;
pub mod docs;

use axum::{Router, routing::get};

use crate::state::AppState;

/// Builds the `/api` router.
///
/// The caller adds the asset fallback and the guard layer around this router.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/config", get(config::get_config))
        .route("/api/tree", get(docs::get_tree))
        .route("/api/docs/{*path}", get(docs::get_doc))
}
