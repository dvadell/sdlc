//! sdlc library crate.

#![forbid(unsafe_code)]

pub mod chat;
pub mod config;
pub mod error;
pub mod guard;
pub mod proposal;
pub mod routes;
pub mod state;
pub mod store;
pub mod watch;

pub use crate::config::{CliArgs, Config, ConfigError};
pub use crate::error::ServerError;
pub use crate::state::AppState;
pub use crate::watch::{EventHub, FileChanged};

use std::io;
use std::net::SocketAddr;

use axum::{Router, middleware};
use tokio::net::TcpListener;

/// Initializes the tracing subscriber from `RUST_LOG`, defaulting to `info`.
///
/// Uses `try_init` so a second call in the same process reports that a global
/// subscriber is already installed instead of panicking; tests may call it freely.
pub fn init_tracing() -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(filter)
        .try_init()
}

/// Binds the loopback listener for the configured port.
///
/// The listener always binds `127.0.0.1` so the server is not reachable from
/// other machines. An already-taken port is reported as [`ServerError::AddrInUse`]
/// with a hint to pick another port.
pub async fn bind(config: &Config) -> Result<TcpListener, ServerError> {
    let port = config.port();

    let listener = TcpListener::bind(("127.0.0.1", port))
        .await
        .map_err(|source| {
            if source.kind() == io::ErrorKind::AddrInUse {
                ServerError::AddrInUse { port }
            } else {
                ServerError::Io { port, source }
            }
        })?;

    Ok(listener)
}

/// Builds the application router for the given state.
///
/// The guard wraps every route, including the fallback, so no request reaches a
/// handler without a valid loopback `Host`/`Origin`. API routes are registered
/// before the fallback; the embedded frontend handler serves everything else,
/// falling back to the entry page.
pub fn router(state: AppState) -> Router {
    let bound_port = state.bound_port;

    routes::router()
        .fallback(routes::assets::serve)
        .layer(middleware::from_fn_with_state(
            guard::GuardState::new(bound_port),
            guard::check,
        ))
        .with_state(state)
}

/// Runs the placeholder server on the listener until Ctrl-C shuts it down.
///
/// The listener must already be bound (tests bind port `0` to get an ephemeral
/// port); the bound address is returned so callers can print or reuse it. The
/// URL is printed only after a successful bind, so it never points at a dead
/// port.
pub async fn serve(config: Config, listener: TcpListener) -> Result<SocketAddr, ServerError> {
    let configured_port = config.port();
    let addr = listener.local_addr().map_err(|source| ServerError::Io {
        port: configured_port,
        source,
    })?;

    let state =
        AppState::new(config, addr.port()).map_err(|source| ServerError::Chat { source })?;

    println!("sdlc listening on http://{addr}");
    println!("{}", ai_status(state.config.as_ref()));

    if state.config.open() {
        open_browser(&format!("http://{addr}"));
    }

    // The watcher starts once the listener is bound and keeps serving even when
    // it fails, so editing a document never depends on file system events.
    let watcher = match watch::start(state.config.project_dir(), &state.events).await {
        Ok(watcher) => Some(watcher),
        Err(error) => {
            tracing::warn!(
                error = %error,
                "the file watcher failed to start; external edits will not be announced"
            );
            None
        }
    };

    let shutdown = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    axum::serve(listener, router(state))
        .with_graceful_shutdown(shutdown)
        .await
        .map_err(|source| ServerError::Io {
            port: configured_port,
            source,
        })?;

    if let Some(watcher) = watcher {
        watcher.abort();
    }

    Ok(addr)
}

/// One startup line describing whether the AI endpoint is configured.
fn ai_status(config: &Config) -> String {
    match config.ai() {
        Some(ai) => format!("AI: configured ({})", ai.base_url()),
        None => "AI: not configured".to_string(),
    }
}

/// Opens the default browser at `url`; failures are warnings and never fatal.
fn open_browser(url: &str) {
    let mut command = std::process::Command::new(browser_program());
    if cfg!(target_os = "windows") {
        command.args(["/C", "start", "", url]);
    } else {
        command.arg(url);
    }
    if let Err(err) = command.spawn() {
        eprintln!("sdlc: could not open browser: {err}");
    }
}

#[cfg(target_os = "macos")]
fn browser_program() -> &'static str {
    "open"
}

#[cfg(target_os = "windows")]
fn browser_program() -> &'static str {
    "cmd"
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn browser_program() -> &'static str {
    "xdg-open"
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn init_tracing_is_safe_to_call_twice() {
        let first = init_tracing();
        let second = init_tracing();
        assert!(first.is_ok());
        assert!(second.is_err());
    }

    #[test]
    fn ai_status_is_not_configured_without_ai_env() {
        let config = Config::from_parts(CliArgs::parse_from(["sdlc"]), |_| None).expect("config");

        assert_eq!(ai_status(&config), "AI: not configured");
    }
}
