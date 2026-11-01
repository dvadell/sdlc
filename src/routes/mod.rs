//! HTTP route modules.

pub mod assets;
pub mod config;

use axum::{Router, routing::get};

use crate::state::AppState;

/// Builds the `/api` router.
///
/// The caller adds the asset fallback and the guard layer around this router.
pub fn router() -> Router<AppState> {
    Router::new().route("/api/config", get(config::get_config))
}
