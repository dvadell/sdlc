//! OpenAI-compatible upstream chat client (ticket 0033).

use std::time::Duration;

use reqwest::Client;
use serde::Serialize;
use serde_json::Value;
use thiserror::Error;

use crate::config::{AiConfig, ApiKey};

/// Errors returned by the upstream chat client.
#[derive(Debug, Error)]
pub enum ChatError {
    /// Chat was used without a configuration.
    #[error("chat is not configured")]
    NotConfigured,

    /// The upstream rejected the credentials.
    #[error("chat upstream rejected the request: unauthorized")]
    Unauthorized,

    /// The upstream answered with a failure status.
    #[error("chat upstream returned status {status}")]
    Upstream {
        /// The failing HTTP status.
        status: u16,
        /// Shortened response body; the full body is never kept.
        snippet: String,
    },

    /// The upstream could not be reached.
    #[error("chat upstream connection failed")]
    Network,

    /// The upstream response could not be understood.
    #[error("chat upstream returned an invalid response")]
    InvalidResponse,

    /// The configured base URL already ends with the completions path.
    #[error("invalid AI base URL {base_url}: it must not end with /chat/completions")]
    InvalidBaseUrl {
        /// The offending base URL.
        base_url: String,
    },
}

/// One conversation turn sent to the upstream model.
#[derive(Debug, Clone, Serialize)]
pub struct ChatMessage {
    /// OpenAI-style role, such as `user` or `assistant`.
    pub role: String,
    /// The message content.
    pub content: String,
}

/// The JSON body sent to the chat completions endpoint.
#[derive(Debug, Serialize)]
struct ChatCompletionRequest<'a> {
    model: &'a str,
    messages: &'a [ChatMessage],
    stream: bool,
}

/// Reusable client for the OpenAI-compatible chat endpoint.
///
/// One instance is built at startup and shared by every chat request, so the
/// underlying connection pool is reused instead of rebuilt per message.
#[derive(Debug)]
pub struct ChatClient {
    /// HTTP client with a connect timeout; no total timeout, streams are long.
    http: Client,
    /// Base URL without a trailing slash and without the endpoint path.
    base_url: String,
    /// Model name sent to the endpoint.
    model: String,
    /// Optional bearer key.
    api_key: Option<ApiKey>,
}

impl ChatClient {
    /// Builds a client from the AI configuration.
    ///
    /// The base URL must not already end with the completions path; a base
    /// that does is rejected with [`ChatError::InvalidBaseUrl`].
    pub fn new(config: &AiConfig) -> Result<Self, ChatError> {
        let base_url = config.base_url().trim_end_matches('/').to_string();
        completions_url(&base_url)?;

        Ok(Self {
            http: http_client()?,
            base_url,
            model: config.model().to_string(),
            api_key: config.api_key(),
        })
    }

    /// Builds a client for an unconfigured chat.
    ///
    /// Sending through it reports [`ChatError::NotConfigured`].
    pub fn unconfigured() -> Self {
        Self {
            http: http_client().expect("the chat HTTP client should always build"),
            base_url: String::new(),
            model: String::new(),
            api_key: None,
        }
    }

    /// Sends the conversation and returns the upstream response.
    ///
    /// The response is streamed; the caller consumes it and relays deltas.
    pub async fn send(&self, messages: &[ChatMessage]) -> Result<reqwest::Response, ChatError> {
        if self.base_url.is_empty() || self.model.is_empty() {
            return Err(ChatError::NotConfigured);
        }

        let url = completions_url(&self.base_url)?;
        let body = ChatCompletionRequest {
            model: &self.model,
            messages,
            stream: true,
        };

        let mut request = self.http.post(&url).json(&body);
        if let Some(api_key) = &self.api_key {
            request = request.bearer_auth(api_key.secret());
        }

        let mut response = request.send().await.map_err(|_| ChatError::Network)?;
        let status = response.status().as_u16();

        if !(200..300).contains(&status) {
            if status == 401 {
                return Err(ChatError::Unauthorized);
            }
            let snippet = response
                .text()
                .await
                .map(|body| truncate(&body))
                .unwrap_or_default();
            return Err(ChatError::Upstream { status, snippet });
        }

        let content_type = response_content_type(&response);
        if content_type.contains("json") {
            // A JSON completion answer is a complete body, so read it and validate
            // it; an unparseable body is an invalid response. Reading takes
            // ownership of the response and a complete JSON answer is not
            // streamed, so a validated body means the completion was well formed.
            // A stalled upstream that stops sending without closing the stream is
            // reported as a network failure once the chunk goes idle.
            let mut body = Vec::new();
            loop {
                let timed_out = tokio::time::timeout(CHUNK_IDLE, response.chunk()).await;
                let chunk = match timed_out {
                    Ok(result) => result.ok().flatten(),
                    Err(_) => break,
                };
                let Some(chunk) = chunk else {
                    break;
                };
                body.extend_from_slice(&chunk);
            }
            serde_json::from_slice::<Value>(&body).map_err(|_| ChatError::InvalidResponse)?;
        }

        Ok(response)
    }
}

/// Joins a base URL and the completions path, trimming trailing slashes.
///
/// A base URL that already ends with the endpoint path would produce a doubled
/// path, so it is rejected with a configuration error instead.
fn completions_url(base_url: &str) -> Result<String, ChatError> {
    let base = base_url.trim_end_matches('/');
    if base.ends_with("/chat/completions") {
        return Err(ChatError::InvalidBaseUrl {
            base_url: base.to_string(),
        });
    }
    Ok(format!("{base}/chat/completions"))
}

/// Builds the shared HTTP client with a connect timeout and no total timeout.
fn http_client() -> Result<Client, ChatError> {
    Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .build()
        .map_err(|_| ChatError::Network)
}

/// Idle time allowed between response body chunks before a stalled upstream
/// stream is reported as a network failure.
const CHUNK_IDLE: Duration = Duration::from_secs(30);

/// The media type of a response, lowercased and without parameters.
fn response_content_type(response: &reqwest::Response) -> String {
    response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(|value| {
            value
                .split(';')
                .next()
                .unwrap_or_default()
                .trim()
                .to_ascii_lowercase()
        })
        .unwrap_or_default()
}

/// Shortens a response snippet so error text stays bounded.
fn truncate(body: &str) -> String {
    body.chars().take(256).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AiConfig;

    fn message(content: &str) -> ChatMessage {
        ChatMessage {
            role: "user".to_string(),
            content: content.to_string(),
        }
    }

    #[test]
    fn completions_url_appends_the_path_without_double_slash() {
        assert_eq!(
            completions_url("https://api.openai.com/v1").expect("the URL should build"),
            "https://api.openai.com/v1/chat/completions"
        );
    }

    #[test]
    fn completions_url_trims_a_trailing_slash() {
        assert_eq!(
            completions_url("https://api.openai.com/v1/").expect("the URL should build"),
            "https://api.openai.com/v1/chat/completions"
        );
    }

    #[test]
    fn completions_url_rejects_a_base_that_already_ends_with_the_endpoint() {
        let err = completions_url("https://api.openai.com/v1/chat/completions")
            .expect_err("a base with the endpoint path must be rejected");

        assert!(matches!(err, ChatError::InvalidBaseUrl { .. }));
    }

    #[test]
    fn new_rejects_a_base_that_already_ends_with_the_endpoint() {
        let err = ChatClient::new(&AiConfig::new(
            "https://api.openai.com/v1/chat/completions".to_string(),
            "gpt-4o".to_string(),
            None,
        ))
        .expect_err("a base with the endpoint path must be rejected");

        assert!(matches!(err, ChatError::InvalidBaseUrl { .. }));
    }

    #[tokio::test]
    async fn unconfigured_client_reports_not_configured() {
        let client = ChatClient::unconfigured();

        let err = client
            .send(&[message("hello")])
            .await
            .expect_err("an unconfigured client must not send");

        assert!(matches!(err, ChatError::NotConfigured));
    }

    #[test]
    fn chat_error_display_never_contains_the_key() {
        let errors = vec![
            ChatError::NotConfigured,
            ChatError::Unauthorized,
            ChatError::Upstream {
                status: 500,
                snippet: "sk-secret-value".to_string(),
            },
            ChatError::Network,
            ChatError::InvalidResponse,
            ChatError::InvalidBaseUrl {
                base_url: "https://api.openai.com/v1".to_string(),
            },
        ];

        for err in errors {
            assert!(!err.to_string().contains("sk-secret-value"), "{err}");
        }
    }
}
