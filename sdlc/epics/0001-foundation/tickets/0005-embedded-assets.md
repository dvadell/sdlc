---
status: done
depends_on: ["0004"]
---

# 0005: Embedded frontend and static asset route

**Epic:** [0001 Foundation and server shell](../description.md) · **Depends on:** 0004 · **Milestone:** M1

## Goal

Serve a placeholder frontend from inside the binary, with no build step. Debug
builds read `frontend/` from disk so a refresh picks up edits; release builds
embed it.

## Build

- `frontend/index.html` and `frontend/style.css`: minimal valid page with a
  heading and a note that the UI is under construction.
- `src/routes/assets.rs` using `rust-embed` on `frontend/`.
- Handler rules: only `GET`/`HEAD`; normalize the request path; reject `..`,
  absolute paths and empty segments; look up the asset; unknown paths fall back
  to `index.html`; paths beginning `/api/` never fall back and return the JSON
  404 shape.
- Content types for `.html`, `.css`, `.js`, `.svg`, `.png`; default
  `application/octet-stream`. Send `Cache-Control: no-store` in debug.
- Wire it as the router fallback, after API routes.

## Already in place

- Server startup (0004), error type (0003).

## Acceptance criteria

- [x] `GET /` returns the page with `text/html`.
- [x] `GET /style.css` returns `text/css`.
- [x] `GET /anything-unknown` returns the page (SPA fallback).
- [x] `GET /api/unknown` returns JSON 404, not HTML.
- [x] `POST /` is rejected (405).
- [x] Encoded traversal cannot read a file outside `frontend/`.

## Tests

- **Integration:** the four GET cases above, plus `%2e%2e/%2e%2e/Cargo.toml`.
- **Integration:** content of `/style.css` matches the file in debug builds.
- **Manual:** edit `style.css`, refresh, see the change without restarting.

## Gotchas

- JavaScript modules need `text/javascript`, or the browser refuses the import.
- `rust-embed` paths are relative to `CARGO_MANIFEST_DIR`; verify the release
  binary does not depend on the working directory.
- Do not add a catch-all fallback that swallows API 404s; test `/api/unknown`.
- Hidden files and dotfiles should not be served even if embedded.
