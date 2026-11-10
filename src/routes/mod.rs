//! HTTP route modules.

pub mod assets;
pub mod chat;
pub mod config;
pub mod docs;
pub mod proposals;

use axum::{
    Router,
    extract::DefaultBodyLimit,
    routing::{get, post},
};

use crate::state::AppState;

/// Maximum accepted save body: 2 MiB, enough for these documents and small
/// enough to fail oversized payloads early.
const MAX_SAVE_BODY_BYTES: usize = 2 * 1024 * 1024;

/// Builds the `/api` router.
///
/// The caller adds the asset fallback and the guard layer around this router.
pub fn router() -> Router<AppState> {
    let body_limit = DefaultBodyLimit::max(MAX_SAVE_BODY_BYTES);
    let docs_route = get(docs::get_doc).put(docs::put_doc).layer(body_limit);
    let create_route = post(docs::create_doc).layer(body_limit);
    let move_route = post(docs::move_doc).layer(body_limit);
    let scaffold_route = post(docs::scaffold).layer(body_limit);
    let chat_route = post(chat::chat).layer(DefaultBodyLimit::max(chat::MAX_CHAT_BODY_BYTES));
    let accept_route = post(proposals::accept).layer(body_limit);
    let reject_route = post(proposals::reject).layer(body_limit);

    // The exact create route is registered before the wildcard read route so
    // `POST /api/docs` never falls through to `/{*path}`.
    Router::new()
        .route("/api/config", get(config::get_config))
        .route("/api/tree", get(docs::get_tree))
        .route("/api/docs", create_route)
        .route("/api/docs/move", move_route)
        .route("/api/scaffold", scaffold_route)
        .route("/api/docs/{*path}", docs_route.delete(docs::delete_doc))
        .route("/api/chat", chat_route)
        .route("/api/proposals/{id}/accept", accept_route)
        .route("/api/proposals/{id}/reject", reject_route)
}
