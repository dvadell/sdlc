---
status: done
depends_on: ["0021"]
---

# 0037: Frontend SSE reader

**Epic:** [0005 AI chat](../description.md) · **Depends on:** 0021 · **Milestone:** M3

## Goal

Read a POST response body incrementally in the browser and turn it into typed
events, without `EventSource` (which cannot send a body).

## Build

- `frontend/js/sse.js` exporting `readSse(response, signal)` as an async
  generator, based on the snippet in `conventions.md` section 11.
- Handle:
  - `\n\n` and `\r\n\r\n` frame separators;
  - multiple `data:` lines joined by `\n`;
  - `event:` defaulting to `message`;
  - comments (`:` lines) ignored;
  - a final buffer with no trailing blank line at EOF;
  - `decoder.decode(value, {stream:true})` plus a final flush;
  - `JSON.parse` failure surfaced as a typed error, not an unhandled rejection.
- Abort via `AbortController`; call `reader.cancel()` and rethrow
  `AbortError` as a distinct "stopped" outcome.
- Add `frontend/sse-selftest.html`: a browser-only page with a few hard-coded
  frames and assertions, so this pure logic can be checked without npm. It is
  not part of the app navigation.
- Add `js/sse.js` to the file-responsibility table in `conventions.md` §11.

## Already in place

- Shell and API module; chat route exists for manual end-to-end.

## Acceptance criteria

- [ ] Frames split at arbitrary byte boundaries still parse.
- [ ] CRLF frames parse.
- [ ] Multi-line data joins with `\n`.
- [ ] A final frame without a blank line is yielded.
- [ ] Abort stops reading and is distinguishable from a network failure.
- [ ] The self-test page passes in Chromium and Firefox.

## Tests

- **Manual (self-test page):** all cases above with hand-built `ReadableStream`s.
- **Manual:** use DevTools throttling to force chunk splits during a real chat.
- **Manual:** stop a stream and confirm no console error is logged as
  unhandled.

## Gotchas

- `TextDecoder` must be created once per stream; creating it per chunk can
  break multi-byte characters.
- `buffer.indexOf("\n\n")` misses CRLF until the `\r` is handled; normalize per
  frame, not the whole buffer, to keep indices simple.
- An empty `data:` payload is valid JSON failure; decide and test.
- `EventSource` is still correct for `/api/events` in 0056; do not replace it.
