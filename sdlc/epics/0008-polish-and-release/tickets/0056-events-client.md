---
status: todo
depends_on: ["0027", "0055"]
---

# 0056: Events SSE route and client reload

**Epic:** [0008 Polish and release](../description.md) · **Depends on:** 0027, 0055 · **Milestone:** M6

## Goal

Deliver `file-changed` events to the page and behave safely: reload a clean
document, ask before touching a dirty one.

## Build

- `GET /api/events` returning `text/event-stream`, one `file-changed` event
  with `{ path }`, plus a comment heartbeat every 15 s. End when the client
  disconnects. No compression.
- `frontend/js/events.js` opens one `EventSource("/api/events")` at startup.
- On event:
  - if the path is the open document and `dirty === false`, fetch and replace
    the editor content/hash, keep the caret if practical;
  - if dirty, show the 0027-style banner with **Reload** and **Keep mine**; do
    not overwrite;
  - if it is another document, refresh the tree silently.
- Ignore events while a save is in flight for that path; the save response owns
  the hash.
- On `EventSource` error, show a small "live updates paused" hint and let the
  browser reconnect; do not spam error bubbles.
- Add a manual "Refresh tree" action as a fallback.
- Add `js/events.js` to the file-responsibility table in `conventions.md` §11.

## Already in place

- Watcher with suppression, conflict banner, tree and editor.

## Acceptance criteria

- [ ] A clean open document updates when changed externally.
- [ ] A dirty document shows the banner and keeps local text.
- [ ] Changes to other documents refresh the tree without stealing focus.
- [ ] The app's own saves produce no banner or reload.
- [ ] Reconnection after a server restart works without a page reload.
- [ ] The page does not busy-loop on reconnect.
- [ ] Heartbeats keep the connection alive behind a local proxy.

## Tests

- **Integration:** connect to `/api/events`, write a file, read one event.
- **Integration:** two events; both arrive in order.
- **Integration:** disconnect and reconnect; no server panic.
- **Manual:** edit the open file in `vi`; watch reload.
- **Manual:** make the editor dirty, edit externally, choose both banner
  options.
- **Manual:** stop and start the server; the page recovers.

## Gotchas

- `EventSource` sends only GET and reconnects automatically; do not implement
  a custom retry loop that duplicates connections.
- A reload caused by an event must not mark the document dirty or trigger a
  save.
- Browser per-host connection limits can matter with a long-lived SSE plus
  chat streams; one events connection is enough.
- The guard must allow the same-origin GET (it has no Origin header in many
  browsers).
- Event floods from `git checkout` are expected; debounce/coalesce in the tree
  refresh.
