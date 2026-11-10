//! Shared application state (ticket 0007).

use std::sync::Arc;

use crate::chat::ChatClient;
use crate::config::Config;
use crate::store::DocStore;

/// State cloned into every handler.
///
/// It holds only cheaply cloneable `Arc`s and `Copy` values; services with
/// interior mutability are added by later tickets.
#[derive(Clone)]
pub struct AppState {
    /// Immutable startup configuration.
    pub config: Arc<Config>,
    /// The document store rooted at the configured project directory.
    pub store: Arc<DocStore>,
    /// The port the listener actually bound, which differs from `config.port()`
    /// when the configured port is `0`.
    pub bound_port: u16,
    /// The upstream chat client, built once at startup when AI is configured.
    pub chat: Arc<ChatClient>,
}

impl AppState {
    /// Creates the state for a server bound to `bound_port`.
    pub fn new(config: Config, bound_port: u16) -> Self {
        // The project directory was validated and canonicalized when the
        // configuration was built, so constructing the store cannot fail for a
        // reason the caller could fix here.
        let store = DocStore::new(config.project_dir().to_path_buf())
            .expect("the configured project directory was canonicalized at startup");

        let chat = match config.ai() {
            Some(ai) => Arc::new(ChatClient::new(ai).expect(
                "the AI base URL must not end with /chat/completions",
            )),
            None => Arc::new(ChatClient::unconfigured()),
        };

        Self {
            config: Arc::new(config),
            store: Arc::new(store),
            bound_port,
            chat,
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
        assert!(Arc::ptr_eq(&state.store, &clone.store));
        assert_eq!(clone.bound_port, 4321);
    }
}
