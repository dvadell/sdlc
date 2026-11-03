---
status: done
depends_on: ["0005"]
---

# 0021: Frontend shell and API module

**Epic:** [0003 Document API and editor](../description.md) · **Depends on:** 0005 · **Milestone:** M1

## Goal

Give the app its permanent three-column shape and one place for every HTTP
call, with no framework and no build step.

## Build

- `frontend/index.html`: header (project name, AI status), `<aside id="sidebar">`,
  `<main id="editor">`, `<aside id="chat">`; `<script type="module" src="/js/main.js">`.
- `frontend/style.css`: CSS grid `260px 1fr 360px`, full height, plain readable
  styles. No framework.
- `frontend/js/state.js`: one `state` object (`config`, `tree`, `current`,
  `dirty`, `chat`, `proposal`). No hidden module-level copies.
- `frontend/js/api.js`:
  - `request(method, path, body)` that sets JSON content type on mutations,
    parses the `{error}` shape and throws an `ApiError` with `code`.
  - Wrappers for config, tree, get/save/create/move/delete/scaffold.
  - `docUrl(path)` that encodes each segment, preserving `/`.
- `frontend/js/main.js`: on load fetch config and tree, render header, call
  placeholder renderers, install global error banner.
- Chat area shows "AI chat arrives in milestone 3" for now.

## Already in place

- Embedded asset route (0005); all HTTP routes (0018–0020).

## Acceptance criteria

- [x] Page loads with no console errors and shows the project name.
- [x] AI status reflects `/api/config`.
- [x] All API functions use relative URLs, never a hard-coded port.
- [x] A failed request shows a readable banner, not an unhandled rejection.
- [x] No `innerHTML` outside the future preview path.
- [x] Works from `http://localhost:<port>` and `http://127.0.0.1:<port>`.

## Tests

- **Manual:** load `/`, check layout at 1280px and 1600px; kill the server and
  confirm the error banner appears.
- **Manual:** inspect the network panel; every mutation carries
  `Content-Type: application/json`.
- **Manual:** a path with a space (if one can be created) is encoded correctly.

## Gotchas

- Modules are cached; after editing a file do a hard reload or disable cache.
- Keep `state` updates in one direction: fetch → assign → render. Do not let
  render functions mutate state.
- `docUrl` must not encode `/`; use `path.split('/').map(encodeURIComponent)`.
- Do not add a bundler, npm or TypeScript. If a helper needs a library, vendor
  it in 0024.
