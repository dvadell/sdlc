//! sdlc library crate.

#![forbid(unsafe_code)]

pub mod config;
pub mod error;

use clap::Parser;

use crate::config::{CliArgs, Config, ConfigError};

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

/// Placeholder entry point for the sdlc binary.
///
/// Returns `Ok(())` until the server shell lands in later tickets.
pub fn run() -> Result<(), ConfigError> {
    let _config = Config::from_parts(CliArgs::parse(), |key| std::env::var(key).ok())?;

    // Server startup consumes the config in later tickets (0004, 0007).
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_tracing_is_safe_to_call_twice() {
        let first = init_tracing();
        let second = init_tracing();
        assert!(first.is_ok());
        assert!(second.is_err());
    }
}
