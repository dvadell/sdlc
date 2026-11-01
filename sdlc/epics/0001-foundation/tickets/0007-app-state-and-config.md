---
status: done
depends_on: ["0002", "0005", "0006"]
---

# 0007: AppState and GET /api/config

**Epic:** [0001 Foundation and server shell](../description.md) · **Depends on:** 0002, 0005, 0006 · **Milestone:** M1

## Goal

Introduce the shared, cheap-to-clone `AppState` all services hang off, and give
the frontend one endpoint that answers "where am I and is AI available?".

## Build

- `src/state.rs`:
  ```rust
  #[derive(Clone)]
  pub struct AppState {
      pub config: Arc<Config>,
      pub bound_port: u16,
      // store, proposals, chat, events are added in later tickets
  }
  ```
- `src/routes/mod.rs` mounting an `/api` router; `src/routes/assets.rs`
  unchanged. Keep the guard layer around the whole app.
- `GET /api/config` DTO:
  `{ project_name, port, ai_configured, model?, ai_host?, version }`.
  `project_name` is the final directory name only, never the absolute path.
  `ai_host` is host and optional port extracted from the base URL, with any
  userinfo stripped.
- No key field, no full base URL if it contains credentials.

## Already in place

- Server, assets, guard (0004–0006); `Config` (0002).

## Acceptance criteria

- [x] `ai_configured` is true only when base URL and model are set.
- [x] `model` appears only when configured.
- [x] The serialized response never contains the API key or userinfo.
- [x] `port` is the actual bound port.
- [x] `project_name` is safe for a non-UTF-8 directory name (lossy fallback).
- [x] Unknown `/api/*` still returns the JSON 404 shape.

## Tests

- **Integration:** config with and without AI env; assert fields.
- **Integration:** key set to a sentinel, assert it is absent from the raw body.
- **Unit:** `ai_host` extraction for `http://localhost:11434/v1`,
  `https://api.example.com/v1`, and a URL with `user:pass@`.
- **Integration:** start with `--port 0`, assert the response port equals the
  bound port.

## Gotchas

- Do not serialize `Config` directly; a DTO keeps internal fields out by
  construction.
- `project_name` for `/` or a root directory has no file name; return a stable
  fallback such as `project`.
- The frontend will bake this response into state at startup; keep the shape
  stable once 0021 consumes it.
