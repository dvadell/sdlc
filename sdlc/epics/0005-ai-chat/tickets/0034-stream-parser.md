---
status: todo
depends_on: ["0033"]
---

# 0034: Upstream streaming parser

**Epic:** [0005 AI chat](../description.md) · **Depends on:** 0033 · **Milestone:** M3

## Goal

Turn the upstream SSE byte stream into text deltas, ending cleanly on
`[DONE]` and reporting malformed events without killing the request.

## Build

- `src/chat/stream.rs`: a function generic over a byte stream so it can be
  tested without HTTP:
  `parse<S>(stream: S) -> impl Stream<Item = Result<String, ChatError>>` using
  `eventsource-stream`.
- For each event, parse `data` as JSON:
  - `[DONE]` (allow surrounding whitespace) ends the stream.
  - `choices[0].delta.content` (string) yields a delta.
  - `choices[0].delta` without content (role-only, finish_reason-only) yields
    nothing.
  - An `error` object yields `ChatError::Upstream`.
  - Invalid JSON yields `ChatError::InvalidResponse` with a short message.
- Ignore comments, keep-alives and unknown event types.
- Do not accumulate the full reply here; 0036/0044 need the accumulation and
  should own it.
- Add `eventsource-stream` and `futures-util` as needed.

## Already in place

- Client that returns a successful streaming `Response` (0033).

## Acceptance criteria

- [ ] Multiple deltas arrive in order.
- [ ] A delta split across two TCP chunks still parses.
- [ ] UTF-8 split across chunks is decoded correctly by the SSE layer.
- [ ] `[DONE]` ends the stream without an error.
- [ ] An empty `choices` array is ignored.
- [ ] Malformed JSON emits one typed error and ends.
- [ ] No panic on an empty stream.

## Tests

- **Unit:** feed `stream::iter` byte chunks for the cases above.
- **Unit:** CRLF frames, `data:` without a space, multi-line data, comments.
- **Unit:** final chunk with `finish_reason` and empty content.
- **Integration (wiremock):** a canned SSE response with three deltas and
  `[DONE]`; collect the deltas.

## Gotchas

- `eventsource-stream` handles event framing, but only if the response content
  type is `text/event-stream`; wiremock must set it.
- OpenAI providers sometimes send a JSON error object as a normal `data` event;
  handle it before assuming `choices`.
- Do not treat `[DONE]` as JSON; it is a sentinel string.
- Dropping the stream must cancel the upstream body; keep ownership simple.
- Avoid `unwrap()` on `serde_json::Value` indexing; missing fields are normal.
