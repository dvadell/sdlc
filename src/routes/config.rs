//! `GET /api/config` (ticket 0007).

use std::path::Path;

use axum::{Json, extract::State};
use serde::Serialize;

use crate::state::AppState;

/// Configuration the frontend needs at startup.
///
/// This DTO is deliberately separate from [`crate::config::Config`]: only the fields listed
/// here can ever be serialized, so the API key and a credentialed base URL
/// cannot leak by accident.
#[derive(Debug, Serialize)]
pub struct ConfigResponse {
    /// Final directory name of the project, never the absolute path.
    project_name: String,
    /// Port the listener actually bound.
    port: u16,
    /// Whether both an AI base URL and model are configured.
    ai_configured: bool,
    /// Model name, present only when the AI is configured.
    #[serde(skip_serializing_if = "Option::is_none")]
    model: Option<String>,
    /// AI host and optional port, present only when the AI is configured.
    #[serde(skip_serializing_if = "Option::is_none")]
    ai_host: Option<String>,
    /// Crate version.
    version: &'static str,
    /// The maximum PRD size in bytes before a size warning is issued (ticket
    /// 0057); the default is 12000 and the override is capped at 1000000.
    prd_max_bytes: u64,
}

impl ConfigResponse {
    /// Builds the response from the shared state.
    fn from_state(state: &AppState) -> Self {
        let ai = state.config.ai();

        Self {
            project_name: project_name(state.config.project_dir()),
            port: state.bound_port,
            ai_configured: ai.is_some(),
            model: ai.map(|ai| ai.model().to_owned()),
            ai_host: ai.and_then(|ai| ai_host(ai.base_url())),
            version: env!("CARGO_PKG_VERSION"),
            prd_max_bytes: state.config.prd_max_bytes(),
        }
    }
}

/// Returns the project directory's final name, or a stable fallback.
///
/// A root or otherwise nameless directory has no file name; non-UTF-8 names
/// are converted lossily so the response stays valid JSON.
fn project_name(project_dir: &Path) -> String {
    project_dir
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "project".to_owned())
}

/// Extracts `host[:port]` from a base URL, discarding any userinfo.
///
/// The base URL is expected to end before `/chat/completions`, but query
/// strings and fragments are ignored defensively. Returns `None` when no
/// authority is present.
fn ai_host(base_url: &str) -> Option<String> {
    let rest = base_url
        .split_once("://")
        .map_or(base_url, |(_, rest)| rest);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);

    (!host.is_empty()).then(|| host.to_owned())
}

/// Handles `GET /api/config`.
pub async fn get_config(State(state): State<AppState>) -> Json<ConfigResponse> {
    Json(ConfigResponse::from_state(&state))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_host_and_port_from_loopback_url() {
        assert_eq!(
            ai_host("http://localhost:11434/v1"),
            Some("localhost:11434".to_owned())
        );
    }

    #[test]
    fn extracts_host_without_a_port() {
        assert_eq!(
            ai_host("https://api.example.com/v1"),
            Some("api.example.com".to_owned())
        );
    }

    #[test]
    fn strips_userinfo_from_the_host() {
        assert_eq!(
            ai_host("https://user:pass@api.example.com/v1"),
            Some("api.example.com".to_owned())
        );
        assert_eq!(
            ai_host("https://user:pass@localhost:11434/v1"),
            Some("localhost:11434".to_owned())
        );
    }

    #[test]
    fn ignores_path_query_and_fragment() {
        assert_eq!(
            ai_host("https://api.example.com:8443/v1?token=x#frag"),
            Some("api.example.com:8443".to_owned())
        );
    }

    #[test]
    fn returns_none_without_an_authority() {
        assert_eq!(ai_host("/"), None);
        assert_eq!(ai_host(""), None);
        assert_eq!(ai_host("http:///v1"), None);
    }

    #[test]
    fn project_name_is_only_the_final_component() {
        assert_eq!(
            project_name(Path::new("/home/person/my-project")),
            "my-project"
        );
    }

    #[test]
    fn root_directory_falls_back_to_project() {
        assert_eq!(project_name(Path::new("/")), "project");
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_directory_name_is_lossily_converted() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        let name = OsStr::from_bytes(b"proj\xffct");
        let converted = project_name(Path::new(name));

        assert_eq!(converted, "proj\u{fffd}ct");
    }
}
