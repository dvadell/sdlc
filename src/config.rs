//! Startup configuration for sdlc.

use std::path::{Path, PathBuf};

use clap::Parser;
use thiserror::Error;

/// Command line arguments for the sdlc binary.
#[derive(Debug, Clone, Default, Parser)]
#[command(version = env!("CARGO_PKG_VERSION"))]
pub struct CliArgs {
    /// Project directory to operate on; stored canonicalized.
    #[arg(long, default_value = ".", value_name = "DIR")]
    pub dir: PathBuf,

    /// Port to listen on; `0` lets the OS pick an ephemeral port.
    #[arg(long, default_value = "9000", value_name = "PORT")]
    pub port: u16,

    /// Open the default browser after the server starts.
    #[arg(long)]
    pub open: bool,
}

/// The default maximum PRD size in bytes before a size warning is issued (ticket
/// 0057).
const DEFAULT_PRD_MAX_BYTES: u64 = 12_000;

/// The upper bound of the `SDLC_PRD_MAX_BYTES` override (ticket 0057).
const PRD_MAX_BYTES_LIMIT: u64 = 1_000_000;

/// Errors from loading startup configuration.
#[derive(Debug, Error)]
pub enum ConfigError {
    /// The configured project directory is missing or not a directory.
    #[error("project directory does not exist or is not a directory: {0}")]
    InvalidDir(String),

    /// Filesystem error while resolving the project directory.
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Immutable startup configuration.
#[derive(Debug)]
pub struct Config {
    /// Canonical project directory.
    project_dir: PathBuf,
    /// Port the server listens on; `0` means ephemeral.
    port: u16,
    /// Whether the browser opens after startup.
    open: bool,
    /// AI endpoint settings; `None` when chat is not configured.
    ai: Option<AiConfig>,
    /// Maximum PRD size in bytes before a size warning is issued (ticket 0057).
    prd_max_bytes: u64,
}

/// Connection settings for the OpenAI-compatible AI endpoint.
#[derive(Debug)]
pub struct AiConfig {
    /// Base URL without a trailing slash.
    base_url: String,
    /// Model name sent to the endpoint.
    model: String,
    /// Optional API key.
    api_key: Option<ApiKey>,
}

impl AiConfig {
    /// Builds an AI endpoint configuration from its parts.
    ///
    /// Whether the result means "configured" is decided by the caller, so
    /// half-built configurations are not representable here.
    pub fn new(base_url: String, model: String, api_key: Option<ApiKey>) -> Self {
        Self {
            base_url,
            model,
            api_key,
        }
    }

    /// The base URL without a trailing slash.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// The model name sent to the endpoint.
    pub fn model(&self) -> &str {
        &self.model
    }

    /// The optional API key.
    pub fn api_key(&self) -> Option<ApiKey> {
        self.api_key.clone()
    }
}

/// API key for the AI endpoint.
///
/// `Debug` is implemented by hand rather than derived so the secret never
/// reaches logs, errors or debug output.
#[derive(Clone)]
pub struct ApiKey(String);

impl ApiKey {
    /// Wraps a raw secret in the newtype.
    pub fn new(secret: String) -> Self {
        Self(secret)
    }

    /// The raw secret; the caller must send it immediately and never log it.
    pub fn secret(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApiKey").field("redacted", &true).finish()
    }
}

impl Config {
    /// Builds an immutable [`Config`] from parsed arguments and an env lookup.
    ///
    /// `lookup_env` is injected so callers (and tests) never touch
    /// process-global environment state.
    pub fn from_parts(
        args: CliArgs,
        lookup_env: impl Fn(&str) -> Option<String>,
    ) -> Result<Self, ConfigError> {
        let project_dir = Self::canonical_dir(&args.dir)?;

        let base_url = lookup_env("SDLC_AI_BASE_URL")
            .map(|raw| {
                let trimmed = raw.trim_end_matches('/');
                if trimmed.is_empty() {
                    raw.to_string()
                } else {
                    trimmed.to_string()
                }
            })
            .unwrap_or_default();
        let model = lookup_env("SDLC_AI_MODEL").unwrap_or_default();
        let api_key = lookup_env("SDLC_AI_API_KEY").map(ApiKey::new);

        let ai = if base_url.is_empty() || model.is_empty() {
            Self::warn_ai_unconfigured(base_url.is_empty(), model.is_empty());
            None
        } else {
            Some(AiConfig {
                base_url,
                model,
                api_key,
            })
        };

        Ok(Self {
            project_dir,
            port: args.port,
            open: args.open,
            ai,
            prd_max_bytes: Self::resolve_prd_max_bytes(&lookup_env),
        })
    }

    /// The canonical project directory.
    pub fn project_dir(&self) -> &Path {
        &self.project_dir
    }

    /// The configured port; `0` means an ephemeral port.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Whether the browser opens after startup.
    pub fn open(&self) -> bool {
        self.open
    }

    /// The AI endpoint settings, when chat is configured.
    pub fn ai(&self) -> Option<&AiConfig> {
        self.ai.as_ref()
    }

    /// The maximum PRD size in bytes before a size warning is issued (ticket
    /// 0057); the default is 12000 bytes and the `SDLC_PRD_MAX_BYTES` override is
    /// capped at 1_000_000 bytes.
    pub fn prd_max_bytes(&self) -> u64 {
        self.prd_max_bytes
    }

    /// Resolves the maximum PRD size in bytes from the environment.
    ///
    /// The `SDLC_PRD_MAX_BYTES` override applies when it is a positive integer
    /// within the upper bound; an absent, malformed, non-positive or over-bound
    /// value falls back to the default, so an unusable override never disables or
    /// unbounded the advisory warning.
    fn resolve_prd_max_bytes(lookup_env: &impl Fn(&str) -> Option<String>) -> u64 {
        let raw = lookup_env("SDLC_PRD_MAX_BYTES").unwrap_or_default();
        let Ok(value) = raw.trim().parse::<u64>() else {
            return DEFAULT_PRD_MAX_BYTES;
        };
        if value == 0 || value > PRD_MAX_BYTES_LIMIT {
            return DEFAULT_PRD_MAX_BYTES;
        }
        value
    }

    /// Validates the project directory and canonicalizes it.
    ///
    /// The canonical form is stored because later tickets join document paths
    /// onto it and must not follow symlinks out of the project root.
    fn canonical_dir(dir: &Path) -> Result<PathBuf, ConfigError> {
        if !dir.is_dir() {
            return Err(ConfigError::InvalidDir(dir.display().to_string()));
        }
        Ok(dir.canonicalize()?)
    }

    /// Warns which AI variables are missing while chat stays disabled.
    fn warn_ai_unconfigured(base_url_missing: bool, model_missing: bool) {
        let mut missing = Vec::new();
        if base_url_missing {
            missing.push("SDLC_AI_BASE_URL");
        }
        if model_missing {
            missing.push("SDLC_AI_MODEL");
        }
        eprintln!("sdlc: AI not configured; missing {}", missing.join(", "));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::{CommandFactory, Parser};
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn env_lookup(entries: &[(&str, Option<String>)]) -> impl Fn(&str) -> Option<String> {
        move |key| {
            entries
                .iter()
                .find_map(|(k, v)| if *k == key { v.clone() } else { None })
        }
    }

    fn args(dir: &str, port: u16, open: bool) -> CliArgs {
        CliArgs {
            dir: PathBuf::from(dir),
            port,
            open,
        }
    }

    fn config_for(args: CliArgs, entries: &[(&str, Option<String>)]) -> Config {
        Config::from_parts(args, env_lookup(entries)).expect("config should build")
    }

    #[test]
    fn defaults_are_dot_port_9000_and_open_off() {
        let config = Config::from_parts(CliArgs::parse_from(["sdlc"]), |_| None).expect("defaults");

        assert_eq!(config.port, 9000);
        assert!(!config.open);
        assert_eq!(
            config.project_dir,
            PathBuf::from(".").canonicalize().expect("cwd")
        );
    }

    #[test]
    fn flag_dir_overrides_the_default() {
        let dir = TempDir::new().expect("temp dir");
        let config = Config::from_parts(
            args(dir.path().to_str().expect("utf-8 path"), 9000, false),
            |_| None,
        )
        .expect("config should build");

        assert_eq!(
            config.project_dir,
            dir.path().canonicalize().expect("canonical dir")
        );
    }

    #[test]
    fn flag_port_overrides_the_default() {
        let config =
            Config::from_parts(args(".", 1234, false), |_| None).expect("config should build");

        assert_eq!(config.port, 1234);
    }

    #[test]
    fn flag_open_overrides_the_default() {
        let config =
            Config::from_parts(args(".", 9000, true), |_| None).expect("config should build");

        assert!(config.open);
    }

    #[test]
    fn version_prints_the_package_version() {
        let command = CliArgs::command();

        assert_eq!(command.get_version(), Some(env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn env_vars_populate_ai_config() {
        let config = config_for(
            args(".", 9000, false),
            &[
                (
                    "SDLC_AI_BASE_URL",
                    Some("https://api.openai.com/v1".to_string()),
                ),
                ("SDLC_AI_MODEL", Some("gpt-4o".to_string())),
            ],
        );

        let ai = config.ai.as_ref().expect("ai should be configured");
        assert_eq!(ai.base_url, "https://api.openai.com/v1");
        assert_eq!(ai.model, "gpt-4o");
    }

    #[test]
    fn only_one_ai_variable_is_not_enough() {
        let config = config_for(
            args(".", 9000, false),
            &[(
                "SDLC_AI_BASE_URL",
                Some("https://api.openai.com/v1".to_string()),
            )],
        );

        assert!(config.ai.is_none());
    }

    #[test]
    fn key_without_url_or_model_is_ignored() {
        let config = config_for(
            args(".", 9000, false),
            &[("SDLC_AI_API_KEY", Some("sk-secret".to_string()))],
        );

        assert!(config.ai.is_none());
    }

    #[test]
    fn key_is_optional() {
        let config = config_for(
            args(".", 9000, false),
            &[
                (
                    "SDLC_AI_BASE_URL",
                    Some("https://api.openai.com/v1".to_string()),
                ),
                ("SDLC_AI_MODEL", Some("gpt-4o".to_string())),
                ("SDLC_AI_API_KEY", Some("sk-secret".to_string())),
            ],
        );

        let ai = config.ai.as_ref().expect("ai should be configured");
        assert!(ai.api_key.is_some());
    }

    #[test]
    fn trailing_slash_on_base_url_is_trimmed() {
        let config = config_for(
            args(".", 9000, false),
            &[
                (
                    "SDLC_AI_BASE_URL",
                    Some("https://api.openai.com/v1/".to_string()),
                ),
                ("SDLC_AI_MODEL", Some("gpt-4o".to_string())),
            ],
        );

        let ai = config.ai.as_ref().expect("ai should be configured");
        assert_eq!(ai.base_url, "https://api.openai.com/v1");
    }

    #[test]
    fn base_url_that_trims_to_empty_is_kept() {
        let config = config_for(
            args(".", 9000, false),
            &[
                ("SDLC_AI_BASE_URL", Some("/".to_string())),
                ("SDLC_AI_MODEL", Some("gpt-4o".to_string())),
            ],
        );

        let ai = config.ai.as_ref().expect("ai should be configured");
        assert_eq!(ai.base_url, "/");
    }

    #[test]
    fn api_key_debug_is_redacted() {
        let key = ApiKey::new("sk-secret".to_string());
        let debug = format!("{key:?}");

        assert!(!debug.contains("sk-secret"));
        assert!(debug.contains("redacted"));
    }

    #[test]
    fn config_debug_does_not_leak_the_key() {
        let config = config_for(
            args(".", 9000, false),
            &[
                (
                    "SDLC_AI_BASE_URL",
                    Some("https://api.openai.com/v1".to_string()),
                ),
                ("SDLC_AI_MODEL", Some("gpt-4o".to_string())),
                ("SDLC_AI_API_KEY", Some("sk-secret".to_string())),
            ],
        );

        let debug = format!("{config:?}");
        assert!(!debug.contains("sk-secret"));
    }

    #[test]
    fn missing_directory_is_rejected_with_readable_error() {
        let err = Config::from_parts(args("/definitely/missing/sdlc/dir", 9000, false), |_| None)
            .expect_err("missing directory should fail");

        assert!(matches!(err, ConfigError::InvalidDir(_)));
        assert!(err.to_string().contains("project directory"));
    }

    #[test]
    fn prd_max_bytes_defaults_to_twelve_thousand() {
        let config = Config::from_parts(CliArgs::parse_from(["sdlc"]), |_| None).expect("defaults");

        assert_eq!(config.prd_max_bytes(), DEFAULT_PRD_MAX_BYTES);
    }

    #[test]
    fn prd_max_bytes_env_override_is_honored() {
        let config = config_for(
            args(".", 9000, false),
            &[("SDLC_PRD_MAX_BYTES", Some("500".to_string()))],
        );

        assert_eq!(config.prd_max_bytes(), 500);
    }

    #[test]
    fn prd_max_bytes_rejects_values_above_the_upper_bound() {
        let config = config_for(
            args(".", 9000, false),
            &[(
                "SDLC_PRD_MAX_BYTES",
                Some((PRD_MAX_BYTES_LIMIT + 1).to_string()),
            )],
        );

        assert_eq!(config.prd_max_bytes(), DEFAULT_PRD_MAX_BYTES);
    }

    #[test]
    fn prd_max_bytes_rejects_non_positive_values() {
        let config = config_for(
            args(".", 9000, false),
            &[("SDLC_PRD_MAX_BYTES", Some("0".to_string()))],
        );

        assert_eq!(config.prd_max_bytes(), DEFAULT_PRD_MAX_BYTES);
    }

    #[test]
    fn prd_max_bytes_accepts_the_upper_bound() {
        let config = config_for(
            args(".", 9000, false),
            &[("SDLC_PRD_MAX_BYTES", Some(PRD_MAX_BYTES_LIMIT.to_string()))],
        );

        assert_eq!(config.prd_max_bytes(), PRD_MAX_BYTES_LIMIT);
    }

    #[test]
    fn prd_max_bytes_falls_back_to_the_default_for_a_malformed_value() {
        let config = config_for(
            args(".", 9000, false),
            &[("SDLC_PRD_MAX_BYTES", Some("not-a-number".to_string()))],
        );

        assert_eq!(config.prd_max_bytes(), DEFAULT_PRD_MAX_BYTES);
    }
}
