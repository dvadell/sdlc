---
status: todo
depends_on: ["0007", "0010", "0013"]
---

# 0018: GET tree and document routes

**Epic:** [0003 Document API and editor](../description.md) · **Depends on:** 0007, 0010, 0013 · **Milestone:** M1

## Goal

Expose the read side of the store over HTTP in the shape the frontend needs.

## Build

- `src/routes/docs.rs`:
  - `GET /api/tree` → `{ layout_exists, docs: [DocEntry] }`.
  - `GET /api/docs/{*path}` → `{ path, content, hash }`.
- Parse the raw wildcard into `DocPath`; do not trust a leading slash the
  extractor may include (normalize once, test both).
- Map store errors through `ApiError`: 404 `not_found`, 400 `not_allowed`,
  500 `io`.
- Send `Cache-Control: no-store` on both routes.
- Mount the router in `AppState`; guard stays in front.

## Already in place

- Store read, tree, `DocPath`, `AppState`, error mapping.

## Acceptance criteria

- [ ] Tree JSON matches the store shape exactly.
- [ ] Document JSON contains exact content and the current hash.
- [ ] Missing document → 404 JSON.
- [ ] `..`, encoded traversal, absolute path and unknown layout → 400.
- [ ] Unknown `/api/*` → JSON 404, not the SPA page.
- [ ] `HEAD` is not accidentally treated as a save.

## Tests

- **Integration:** tree from a fixture project; content and hash from a read.
- **Integration:** modify the file on disk, read again, hash changes.
- **Security:** `/api/docs/../Cargo.toml`, `%2e%2e%2fCargo.toml`,
  `/api/docs/%2Fetc%2Fpasswd`.
- **Integration:** invalid UTF-8 file → 400/500 with a stable code, no panic.

## Gotchas

- Axum wildcard extractors have changed between versions; write one test that
  asserts the captured string for a normal path before building on it.
- A URL-encoded slash may or may not be decoded by the time the extractor sees
  it. Reject anything that is not an exact valid `DocPath` after one decode.
- Do not include absolute paths or OS error text in responses; log them.
- The frontend will treat `hash` as opaque; never derive meaning from it there.
