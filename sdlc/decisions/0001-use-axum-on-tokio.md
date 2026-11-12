---
status: accepted
---

# 0001: Use axum on tokio

## Context

The server has to serve the embedded frontend, the document API and the chat API
before any of them is useful (see architecture sections 1 and 3).

## Decision

Use `axum` on `tokio`.

## Consequences

- The HTTP stack is the standard Rust web-library split, so handlers and services
  stay async from the first request.
- The rest of the architecture already assumes a multi-threaded async runtime and
  `tokio::task::spawn_blocking` for filesystem work.
- The library choice lives in `sdlc/conventions.md` and here instead of being
  re-litigated per ticket.
