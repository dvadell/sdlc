---
status: done
depends_on: ["0001"]
---

# 0003: API error type and logging

**Epic:** [0001 Foundation and server shell](../description.md) · **Depends on:** 0001 · **Milestone:** M1

## Goal

Give every route one error shape and one place that decides HTTP status, while
keeping internal details out of responses.

## Build

- `src/error.rs`:
  - `ApiError { status: StatusCode, code: &'static str, message: String }`.
  - Constructors `bad_request`, `not_found`, `conflict`, `forbidden`,
    `unsupported_media_type`, and `internal(cause)`.
  - `IntoResponse` producing `{"error":{"code":"...","message":"..."}}` with
    `Content-Type: application/json`.
  - `internal` logs the cause with `tracing::error!` and returns a generic
    message. It must not contain absolute paths or upstream response bodies.
- `src/lib.rs` exposes `init_tracing()`: `tracing-subscriber` with `RUST_LOG`,
  defaulting to `info`; use `try_init` so tests can call it.
- Add `axum`, `serde_json`, `tracing`, `tracing-subscriber`, `thiserror`.

## Already in place

- Crate skeleton from 0001.

## Acceptance criteria

- [x] Error body has exactly `error.code` and `error.message`.
- [x] Status and code are stable and chosen in one match, not in handlers.
- [x] Internal errors are logged once and expose no filesystem path.
- [x] `init_tracing()` is safe to call twice.

## Tests

- **Unit:** serialization of each constructor (status, code, JSON keys).
- **Unit:** `internal("secret /home/me/file")` response does not contain the
  path while the log call receives it.
- **Unit:** message survives JSON escaping (quotes, newlines).
- **Manual:** start with `RUST_LOG=debug` and confirm logs go to stderr.

## Gotchas

- Do not put `current_hash` or other details in the message yet; 0019 needs a
  conflict code only, and the UI can reload with `GET`.
- `code` is an API contract: use snake_case and never derive it from
  `Display`.
- Adding `axum` here is fine; 0004 uses the same version from `Cargo.toml`.

## Notes

- Crate context: `sdlc::run() -> Result<(), ConfigError>` in `src/lib.rs` is the
  current entry point; server startup (0004) will call `init_tracing()` and
  consume `Config`, so keep the field set stable for those tickets.
- No module error enums exist yet; `ApiError` must pick a fixed
  `(StatusCode, &'static str)` per kind in a single match inside `src/error.rs`.
  Nothing to convert yet, so the constructors carry the mapping.
- `thiserror = "2"` is already in `Cargo.toml`; keep it. Add `axum` pinned at
  `0.8` (see `sdlc/conventions.md`), plus `serde_json`, `tracing`,
  `tracing-subscriber` (with the `env-filter` feature).
- `internal(cause)` accepts anything `Debug + Send + Sync + 'static` (strings
  included) so tests can pass `"secret /home/me/file"`.
