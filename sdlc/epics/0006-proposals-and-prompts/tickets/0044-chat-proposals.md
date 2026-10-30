---
status: todo
depends_on: ["0036", "0041"]
---

# 0044: Wire chat replies into proposals

**Epic:** [0006 Proposals and prompts](../description.md) · **Depends on:** 0036, 0041 · **Milestone:** M4

## Goal

When a reply finishes cleanly, parse its blocks, create a proposal and send it
to the browser as an SSE event. Warnings and validation failures are visible.

## Build

- In the chat route, accumulate every delta (already done in 0036).
- After the upstream stream ends without error:
  - parse blocks (0040);
  - if warnings exist, emit a new `warning` event `{message}` (one per warning)
    before the proposal;
  - if there are blocks, call `ProposalService::create` (0041);
  - success → `proposal` event `{id, changes:[{path, action, diff}], warnings}`;
  - invalid path/duplicate/empty → `error` event with a clear code and message;
  - then `done`.
- Never parse a partial reply from a stream that ended in `error` or was
  aborted.
- If no blocks, emit only `done`; ordinary chat text was already streamed.
- Add `warning` to the chat stream event table in `architecture.md` (the
  document currently lists only delta/proposal/error/done).

## Already in place

- Chat stream, parser, proposal service.

## Acceptance criteria

- [ ] A reply with one `FILE` block emits `delta*`, `proposal`, `done`.
- [ ] Proposal data contains relative paths, action and diff text.
- [ ] Malformed block emits `warning`, then `done`, and no proposal.
- [ ] Invalid path emits `error` and no proposal; no files change.
- [ ] A reply with no blocks emits no proposal.
- [ ] An upstream error or client abort never creates a proposal.
- [ ] A feature proposal plus index refresh happens only on accept, not here.

## Tests

- **Integration (wiremock):** full reply with two blocks and a delete; assert
  event order and payload.
- **Integration:** unterminated block; warning then done.
- **Integration:** path `../../etc/passwd`; error event, proposal registry
  empty.
- **Integration:** upstream cuts off after deltas; no proposal event.
- **Unit:** route-to-event mapping for the new warning variant.

## Gotchas

- Proposal creation reads files; it happens after the model finishes, so the
  stream may pause briefly before `proposal`. Send a comment or keep the
  connection alive if it becomes noticeable.
- `proposal` data is built once; do not stream the diff separately.
- Do not emit `done` after an `error`; conventions say the stream ends with
  one terminal event.
- The proposal's paths are relative; the UI must not prepend `/`.
- The system prompt and parser must agree on delimiters; add a test that the
  prompt contains the exact marker text.
