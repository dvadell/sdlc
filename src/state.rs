//! Shared application state (ticket 0007).

use std::sync::Arc;

use crate::config::Config;

/// State cloned into every handler.
///
/// It holds only cheaply cloneable `Arc`s and `Copy` values; services with
/// interior mutability are added by later tickets.
#[derive(Clone)]
pub struct AppState {
    /// Immutable startup configuration.
    pub config: Arc<Config>,
    /// Port the listener actually bound, which differs from `config.port()`
    /// when the configured port is `0`.
    pub bound_port: u16,
}

impl AppState {
    /// Creates the state for a server bound to `bound_port`.
    pub fn new(config: Config, bound_port: u16) -> Self {
        Self {
            config: Arc::new(config),
            bound_port,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::CliArgs;
    use std::path::PathBuf;

    #[test]
    fn clones_share_the_config_and_keep_the_bound_port() {
        let config = Config::from_parts(
            CliArgs {
                dir: PathBuf::from("/tmp"),
                port: 0,
                open: false,
            },
            |_| None,
        )
        .expect("config should build");
        let state = AppState::new(config, 4321);
        let clone = state.clone();

        assert!(Arc::ptr_eq(&state.config, &clone.config));
        assert_eq!(clone.bound_port, 4321);
    }
}
