//! Host and Origin guard middleware (ticket 0006).
//!
//! The server is loopback-only, but a malicious web page can still make the
//! browser send requests to it. Requiring the exact bound `Host`, an optional
//! same-origin `Origin`, and JSON content types for state-changing methods
//! blocks DNS rebinding and cross-site requests without affecting the app's own
//! requests.

use axum::{
    extract::{Request, State},
    http::{HeaderMap, Method, header},
    middleware::Next,
    response::{IntoResponse, Response},
};

use crate::error::ApiError;

/// Shared guard state: the port the listener actually bound.
///
/// The configured port may be `0`, so the real port is passed in after the
/// listener has been bound.
#[derive(Debug, Clone, Copy)]
pub struct GuardState {
    port: u16,
}

impl GuardState {
    /// Creates guard state for the port the server is listening on.
    pub fn new(port: u16) -> Self {
        Self { port }
    }
}

/// Rejects requests that do not come from the app's own origin.
pub async fn check(State(state): State<GuardState>, request: Request, next: Next) -> Response {
    if let Err(err) = validate(&state, request.method(), request.headers()) {
        return err.into_response();
    }

    next.run(request).await
}

/// Validates the `Host`, `Origin` and `Content-Type` headers of a request.
fn validate(state: &GuardState, method: &Method, headers: &HeaderMap) -> Result<(), ApiError> {
    check_host(headers, state.port)?;
    check_origin(headers, state.port)?;
    check_content_type(method, headers)?;
    Ok(())
}

/// Requires exactly one `Host` header naming the bound loopback address.
fn check_host(headers: &HeaderMap, port: u16) -> Result<(), ApiError> {
    let mut hosts = headers.get_all(header::HOST).iter();
    let Some(host) = hosts.next() else {
        return Err(ApiError::forbidden());
    };
    if hosts.next().is_some() {
        return Err(ApiError::forbidden());
    }

    let host = host.to_str().map_err(|_| ApiError::forbidden())?;
    if is_allowed_host(host.trim(), port) {
        Ok(())
    } else {
        Err(ApiError::forbidden())
    }
}

/// Accepts an absent `Origin`, but requires a present one to be same-origin.
fn check_origin(headers: &HeaderMap, port: u16) -> Result<(), ApiError> {
    let mut origins = headers.get_all(header::ORIGIN).iter();
    let Some(origin) = origins.next() else {
        return Ok(());
    };
    if origins.next().is_some() {
        return Err(ApiError::forbidden());
    }

    let origin = origin.to_str().map_err(|_| ApiError::forbidden())?;
    if is_allowed_origin(origin.trim(), port) {
        Ok(())
    } else {
        Err(ApiError::forbidden())
    }
}

/// Requires JSON for methods that can change state.
fn check_content_type(method: &Method, headers: &HeaderMap) -> Result<(), ApiError> {
    if !is_state_changing(method) {
        return Ok(());
    }

    let mut content_types = headers.get_all(header::CONTENT_TYPE).iter();
    let Some(content_type) = content_types.next() else {
        return Err(ApiError::unsupported_media_type());
    };
    if content_types.next().is_some() {
        return Err(ApiError::unsupported_media_type());
    }

    let content_type = content_type
        .to_str()
        .map_err(|_| ApiError::unsupported_media_type())?;
    if is_json_content_type(content_type) {
        Ok(())
    } else {
        Err(ApiError::unsupported_media_type())
    }
}

/// Reports whether `method` can change server state.
fn is_state_changing(method: &Method) -> bool {
    matches!(
        *method,
        Method::POST | Method::PUT | Method::PATCH | Method::DELETE
    )
}

/// Matches `127.0.0.1:<port>` or `localhost:<port>`, ignoring host case.
fn is_allowed_host(value: &str, port: u16) -> bool {
    let Some((host, port_text)) = value.rsplit_once(':') else {
        return false;
    };

    let host_matches = host == "127.0.0.1" || host.eq_ignore_ascii_case("localhost");
    let port_matches = !port_text.is_empty()
        && port_text.bytes().all(|byte| byte.is_ascii_digit())
        && port_text.parse::<u16>() == Ok(port);

    host_matches && port_matches
}

/// Matches an `http://` origin whose host and port are the bound loopback one.
fn is_allowed_origin(value: &str, port: u16) -> bool {
    let Some((scheme, rest)) = value.split_once("://") else {
        return false;
    };

    scheme.eq_ignore_ascii_case("http") && is_allowed_host(rest, port)
}

/// Matches `application/json` with optional parameters such as `charset`.
fn is_json_content_type(value: &str) -> bool {
    value
        .split(';')
        .next()
        .is_some_and(|media_type| media_type.trim().eq_ignore_ascii_case("application/json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PORT: u16 = 9000;

    #[test]
    fn allows_both_loopback_host_forms() {
        assert!(is_allowed_host("127.0.0.1:9000", PORT));
        assert!(is_allowed_host("localhost:9000", PORT));
        assert!(is_allowed_host("LOCALHOST:9000", PORT));
    }

    #[test]
    fn rejects_wrong_hosts_and_ports() {
        assert!(!is_allowed_host("evil.example:9000", PORT));
        assert!(!is_allowed_host("127.0.0.1:9001", PORT));
        assert!(!is_allowed_host("127.0.0.1", PORT));
        assert!(!is_allowed_host("[::1]:9000", PORT));
        assert!(!is_allowed_host("localhost:9000/", PORT));
    }

    #[test]
    fn allows_only_http_loopback_origins() {
        assert!(is_allowed_origin("http://127.0.0.1:9000", PORT));
        assert!(is_allowed_origin("http://localhost:9000", PORT));
        assert!(is_allowed_origin("HTTP://LocalHost:9000", PORT));
    }

    #[test]
    fn rejects_foreign_https_null_and_path_origins() {
        assert!(!is_allowed_origin("http://evil.test", PORT));
        assert!(!is_allowed_origin("https://127.0.0.1:9000", PORT));
        assert!(!is_allowed_origin("null", PORT));
        assert!(!is_allowed_origin("http://localhost:9000/", PORT));
    }

    #[test]
    fn accepts_json_with_parameters_case_insensitively() {
        assert!(is_json_content_type("application/json"));
        assert!(is_json_content_type("Application/JSON; Charset=UTF-8"));
        assert!(!is_json_content_type("text/plain"));
        assert!(!is_json_content_type("application/json-patch+json"));
    }
}
