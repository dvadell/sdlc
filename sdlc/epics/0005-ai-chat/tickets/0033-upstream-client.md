---
status: todo
depends_on: ["0002", "0003"]
---

# 0033: OpenAI-compatible client

**Epic:** [0005 AI chat](../description.md) · **Depends on:** 0002, 0003 · **Milestone:** M3

## Goal

Build and send the upstream chat request with the configured base URL, model
and optional bearer key, and turn HTTP failures into one typed error.

## Build

- `src/chat/client.rs` with `ChatClient { http: reqwest::Client, base_url: String,
  model: String, api_key: Option<ApiKey> }`, constructed once at startup when
  AI is configured.
- `send(messages) -> Result<reqwest::Response, ChatError>`:
  - `POST {base_url}/chat/completions` with
    `{ model, messages, stream: true }`.
  - `Authorization: Bearer <key>` only when a key is set.
  - `Content-Type: application/json`.
- Join URLs without double slashes; trim a trailing `/` from the base URL.
- `ChatError`: `NotConfigured`, `Unauthorized`, `Upstream { status, snippet }`
  (snippet truncated, no full body), `Network`, `InvalidResponse`.
- Set a connect timeout (10 s) but no total timeout; streams can be long.
- Add `reqwest` with rustls and the `stream` feature.

## Already in place

- `AiConfig`, redacted key, `ApiError`.

## Acceptance criteria

- [ ] Request path is exactly `{base}/chat/completions`.
- [ ] Model and `stream: true` are in the body.
- [ ] No `Authorization` header when the key is absent.
- [ ] 401 maps to `Unauthorized`; other non-2xx map to `Upstream` with status.
- [ ] Connection failure maps to `Network`, not a panic.
- [ ] The key is never logged or included in an error message.

## Tests

- **Unit:** URL joining for base with and without trailing slash; base already
  ending in `/chat/completions` is rejected with a clear config error.
- **Integration (wiremock):** assert method, path, headers and JSON body.
- **Integration:** 401, 500, invalid JSON response, connection refused.
- **Security:** assert the error `Display` of all variants lacks the key.

## Gotchas

- `reqwest::Client` should be reused; constructing one per request loses
  connection pooling and is easy to overlook.
- Some local servers need no key; do not send an empty bearer header.
- The base URL may contain a path (`/v1`), so append, do not replace the path.
- Do not enable redirects to other hosts blindly; keep the default and note it.
- Never dump the request body on error; it contains document text.
