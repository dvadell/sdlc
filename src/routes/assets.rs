//! Embedded frontend asset serving (ticket 0005).

use axum::{
    Router,
    http::{HeaderMap, HeaderValue, Method, StatusCode, Uri, header},
    response::{IntoResponse, Response},
};
use rust_embed::Embed;
use std::borrow::Cow;

use crate::error::ApiError;

/// Embedded frontend files from the `frontend/` folder.
#[derive(Embed)]
#[folder = "frontend"]
struct Asset;

/// Serves the embedded frontend; unmatched paths fall back to the entry page.
pub fn router() -> Router {
    Router::new().fallback(serve)
}

/// Fallback handler for the embedded frontend.
///
/// Only `GET` and `HEAD` are served. Asset paths are normalized before lookup
/// so traversal attempts are rejected, and `/api/*` paths answer with the JSON
/// error shape instead of falling back to the page.
pub async fn serve(method: Method, uri: Uri) -> Response {
    if method != Method::GET && method != Method::HEAD {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }

    let path = percent_decode(uri.path()).unwrap_or_default();

    if is_api_path(&path) {
        return ApiError::not_found().into_response();
    }

    let Some(key) = asset_key(&path) else {
        // Empty segments, absolute paths and `..` are rejected, not resolved.
        return StatusCode::NOT_FOUND.into_response();
    };

    let file = Asset::get(&key).or_else(|| Asset::get("index.html"));

    let Some(file) = file else {
        // The embedded entry page always ships with the binary.
        return StatusCode::NOT_FOUND.into_response();
    };

    let mut headers = HeaderMap::new();
    let content_type = HeaderValue::from_str(file.metadata.mimetype())
        .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream"));
    headers.insert(header::CONTENT_TYPE, content_type);
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static(cache_control()),
    );

    let body: Cow<'static, [u8]> = if method == Method::HEAD {
        Cow::Borrowed(&[])
    } else {
        file.data
    };

    (StatusCode::OK, headers, body).into_response()
}

/// Maps a normalized request path to an asset key inside `frontend/`.
///
/// Returns `None` for paths that must not resolve (empty, absolute, empty
/// segments, `..`, dotfiles) so the caller rejects them instead of serving them.
fn asset_key(path: &str) -> Option<String> {
    if path.is_empty() || !path.starts_with('/') {
        return None;
    }

    let trimmed = &path[1..];
    if trimmed.is_empty() {
        return Some("index.html".to_string());
    }

    let segments: Vec<&str> = trimmed.split('/').collect();

    if segments.iter().any(|segment| segment.is_empty()) {
        return None;
    }

    if segments.contains(&"..") {
        return None;
    }

    if segments.iter().any(|segment| segment.starts_with('.')) {
        return None;
    }

    Some(segments.join("/"))
}

/// Reports whether a request path addresses the API, which never falls back.
fn is_api_path(path: &str) -> bool {
    path == "/api" || path.starts_with("/api/")
}

/// Percent-decodes a request path, or returns `None` when it is not UTF-8.
fn percent_decode(input: &str) -> Option<String> {
    let bytes = input.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).ok()?;
            let byte = u8::from_str_radix(hex, 16).ok()?;
            decoded.push(byte);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }

    String::from_utf8(decoded).ok()
}

/// `no-store` while debugging so a browser refresh picks up edits; immutable
/// caching once the assets are embedded in a release binary.
fn cache_control() -> &'static str {
    if cfg!(debug_assertions) {
        "no-store"
    } else {
        "public, max-age=31536000, immutable"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_encoded_traversal() {
        assert_eq!(
            percent_decode("/%2e%2e/%2e%2e/Cargo.toml"),
            Some("/../../Cargo.toml".to_string())
        );
    }

    #[test]
    fn rejects_empty_segments() {
        assert_eq!(asset_key("//style.css"), None);
        assert_eq!(asset_key("/style.css/"), None);
    }

    #[test]
    fn rejects_traversal() {
        assert_eq!(asset_key("/../Cargo.toml"), None);
        assert_eq!(asset_key("/a/../../b"), None);
    }

    #[test]
    fn rejects_hidden_files() {
        assert_eq!(asset_key("/.hidden"), None);
        assert_eq!(asset_key("/.config/style.css"), None);
    }

    #[test]
    fn resolves_index_and_assets() {
        assert_eq!(asset_key("/"), Some("index.html".to_string()));
        assert_eq!(asset_key("/style.css"), Some("style.css".to_string()));
        assert_eq!(asset_key("/js/app.js"), Some("js/app.js".to_string()));
    }

    #[test]
    fn reports_api_paths() {
        assert!(is_api_path("/api/unknown"));
        assert!(is_api_path("/api"));
        assert!(!is_api_path("/style.css"));
        assert!(!is_api_path("/"));
    }
}
