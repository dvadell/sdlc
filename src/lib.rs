//! sdlc library crate.

#![forbid(unsafe_code)]

pub mod config;

use clap::Parser;

use crate::config::{CliArgs, Config, ConfigError};

/// Placeholder entry point for the sdlc binary.
///
/// Returns `Ok(())` until the server shell lands in later tickets.
pub fn run() -> Result<(), ConfigError> {
    let _config = Config::from_parts(CliArgs::parse(), |key| std::env::var(key).ok())?;

    // Server startup consumes the config in later tickets (0004, 0007).
    Ok(())
}
