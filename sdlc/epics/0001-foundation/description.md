---
status: planned
---

# 0001: Foundation and server shell

*Tickets 0001–0007 · Milestone M1*

## Outcome

A Rust binary that starts on `127.0.0.1`, rejects requests a malicious page
could send, serves an embedded placeholder UI, and exposes configuration the
frontend needs. No document editing yet.

## Why this order

The guard and the configuration boundary are cheapest to add before any route
exists. Every later epic assumes `just check` works, `AppState` is cloneable,
and errors have one JSON shape.

## Tickets

| # | Title | Depends on |
|---|-------|------------|
| [0001](tickets/0001-project-skeleton.md) | Project skeleton and required tooling | none |
| [0002](tickets/0002-configuration.md) | CLI and environment configuration | 0001 |
| [0003](tickets/0003-errors-and-logging.md) | API error type and logging | 0001 |
| [0004](tickets/0004-server-startup.md) | Loopback server startup | 0002, 0003 |
| [0005](tickets/0005-embedded-assets.md) | Embedded frontend and static asset route | 0004 |
| [0006](tickets/0006-guard.md) | Host and Origin guard middleware | 0004 |
| [0007](tickets/0007-app-state-and-config.md) | AppState and GET /api/config | 0002, 0005, 0006 |

## Exit criteria

- `cargo run -- --port 0` prints the actual URL and starts.
- `GET /` returns the placeholder page; unknown `/api/*` paths return the JSON
  error shape, not the page.
- Requests with a foreign `Host` or `Origin` are rejected.
- `GET /api/config` reports `ai_configured` correctly and never includes the API
  key.
- `just check` passes.

## Risks

- The installed toolchain may be older than edition 2024 requires; settle that
  in 0001 before writing features.
- Guard rules are easy to make too strict and break `EventSource` later; keep
  the allowed set narrow but test the same-origin cases now.
