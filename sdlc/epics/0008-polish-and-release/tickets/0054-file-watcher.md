---
status: todo
depends_on: ["0002"]
---

# 0054: File watcher and event hub

**Epic:** [0008 Polish and release](../description.md) · **Depends on:** 0002 · **Milestone:** M6

## Goal

Notice when another tool changes a project document and publish one debounced
event that any number of SSE clients can receive.

## Build

- `src/watch.rs`: start a `notify` watcher with a debouncer (300 ms) on the
  project root.
- Filter events through `DocPath`: ignore `target/`, `.git/`, temp files, and
  anything outside the layout. Rename events may produce two paths; emit each.
- `EventHub` wrapping `tokio::sync::broadcast` of
  `FileChanged { path: String }`; capacity around 64. A lagging receiver gets a
  resync signal, not a panic.
- Start the watcher after the server binds; log a warning if watching fails
  (editing must still work).
- Expose `subscribe()` for the SSE route.
- Debounce per path so one editor save produces one event.

## Already in place

- Config/project root, `DocPath`, logging.

## Acceptance criteria

- [ ] Writing a document emits exactly one event after the debounce.
- [ ] Two writes within the window still produce one event (or two, if
      separated; document the window).
- [ ] Non-document files and `target/` are ignored.
- [ ] Multiple subscribers all receive the event.
- [ ] A dropped subscriber does not stop the watcher.
- [ ] Watcher startup failure is non-fatal and visible in logs.

## Tests

- **Integration (Unix):** temp project, subscribe, write a file, receive within
  2 s; assert one event.
- **Integration:** write `Cargo.toml` and a file in `target/`; no event.
- **Integration:** two subscribers; both receive one event.
- **Unit:** path filter accepts each valid kind and rejects others.
- **Manual:** save from `vi` and from VS Code; one notification each.

## Gotchas

- Editors often write via rename, producing create/remove pairs; debounce and
  de-duplicate by path.
- `notify` is platform-specific; CI is likely Linux only. Gate timing tests or
  keep timeouts generous.
- Do not watch the whole filesystem; watch the root but filter aggressively.
- Keep the broadcast send non-blocking; a slow client must not stall writes.
- The watcher has no suppression yet; 0055 is required before wiring the UI.
