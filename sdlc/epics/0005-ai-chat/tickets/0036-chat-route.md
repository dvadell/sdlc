---
status: todo
depends_on: ["0007", "0034", "0035"]
---

# 0036: Chat SSE route

**Epic:** [0005 AI chat](../description.md) · **Depends on:** 0007, 0034, 0035 · **Milestone:** M3

## Goal

Accept a conversation, load the referenced documents, call the model and relay
deltas as SSE events that always terminate.

## Build

- `POST /api/chat` body:
  `{ messages: [{role, content}], open_doc: path, attachments: [path] }`.
- Validate: non-empty messages, roles only `user`/`assistant`, sizes bounded,
  every path parses as `DocPath`; reject a missing open document with 400.
- If AI is not configured, return 503 `ai_not_configured` as JSON before
  opening a stream.
- Build the prompt (0035), call the client (0033), parse (0034), and return
  `text/event-stream`:
  - `delta` `{text}` per chunk;
  - `error` `{code, message}` on failure, then end;
  - `done` `{}` on clean end.
- Accumulate the full reply in the route for 0044, but do not parse proposals
  yet.
- Drop the upstream response when the client disconnects; do not spawn a task
  that outlives the response.
- Add `Cache-Control: no-cache`, `X-Accel-Buffering: no` and no compression.

## Already in place

- Client, parser, prompt builder, `AppState`, guard.

## Acceptance criteria

- [ ] A fake server's three deltas appear as three `delta` events in order.
- [ ] The stream ends with `done` and no extra data.
- [ ] Upstream 401/500 produces one `error` event and ends.
- [ ] An invalid request returns JSON 400, never an SSE stream.
- [ ] Unconfigured returns 503 and the UI-visible code.
- [ ] EventSource-style `event:`/`data:` framing is valid.
- [ ] A client abort cancels the upstream request promptly.

## Tests

- **Integration (wiremock):** success, upstream error, malformed stream.
- **Integration:** bad path, missing open doc, system role, unconfigured.
- **Integration:** read raw bytes and assert event order and content type.
- **Integration:** drop the response body mid-stream and assert the mock sees a
  closed connection (best effort; skip if flaky).

## Gotchas

- Compression middleware buffers SSE; do not add it to this route.
- `Event::json_data` can fail only on serialization; always emit a valid frame.
- The route must not hold the store lock while streaming; load documents first.
- Log upstream status and a short message, never the full reply or documents.
- The guard requires JSON on POST; browsers set it, curl tests must too.
