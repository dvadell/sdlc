//! Shared application state (ticket 0007).

use std::sync::Arc;

use crate::chat::{ChatClient, ChatError};
use crate::config::Config;
use crate::proposal::ProposalService;
use crate::store::DocStore;

/// State cloned into every handler.
///
/// It holds only cheaply cloneable `Arc`s and `Copy` values; the proposal
/// service adds the interior mutability of its in-memory proposal map.
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
    /// The in-memory proposal service that turns chat blocks into proposals.
    pub proposals: Arc<ProposalService>,
}

impl AppState {
    /// Creates the state for a server bound to `bound_port`.
    ///
    /// The AI chat client is built from the configuration, so a bad AI base
    /// URL is a startup error the caller reports instead of an expectation that
    /// would panic the process.
    pub fn new(config: Config, bound_port: u16) -> Result<Self, ChatError> {
        // The project directory was validated and canonicalized when the
        // configuration was built, so constructing the store cannot fail for a
        // reason the caller could fix here.
        let store = DocStore::new(config.project_dir().to_path_buf())
            .expect("the configured project directory was canonicalized at startup");
        let store = Arc::new(store);

        let chat = match config.ai() {
            Some(ai) => Arc::new(ChatClient::new(ai)?),
            None => Arc::new(ChatClient::unconfigured()),
        };

        let proposals = Arc::new(ProposalService::new(store.clone()));

        Ok(Self {
            config: Arc::new(config),
            store,
            bound_port,
            chat,
            proposals,
        })
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
        let state = AppState::new(config, 4321).expect("the state should build");
        let clone = state.clone();

        assert!(Arc::ptr_eq(&state.config, &clone.config));
        assert!(Arc::ptr_eq(&state.store, &clone.store));
        assert!(Arc::ptr_eq(&state.proposals, &clone.proposals));
        assert_eq!(clone.bound_port, 4321);
    }
}
