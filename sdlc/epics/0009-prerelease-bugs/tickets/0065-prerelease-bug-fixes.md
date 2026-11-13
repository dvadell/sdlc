---
status: done
depends_on: ["0038", "0061"]
---

# 0065: Prerelease bug fixes

**Epic:** [0009 Prerelease bugs](../description.md) · **Depends on:** 0038 · **Milestone:** M6

## Goal

Fix the defects found while exercising the release candidate before calling v1
done: the AI chat send control stayed disabled while the composer held text, a
streamed AI reply that ended in a proposal refusal lost its text, and the server
did not stop on Ctrl-C while a client held the file-change SSE stream open.

## Build

- Recompute the send control from the `canSend` derivation on composer edits
  (input events) instead of only on panel renders, so opening a document and
  filling the composer re-enables it; clearing the composer or closing the
  document disables it again.
- Keep the assistant bubble of a reply that already streamed text when the reply
  ends in a proposal refusal, adding the error bubble after it; drop the assistant
  bubble only when the reply streamed no text.
- Abort the axum serve task when the Ctrl-C shutdown signal fires instead of
  waiting out the graceful shutdown of held-open client connections, so the server
  stops immediately or within one second.

## Acceptance criteria

- [x] Typing with a document open and the AI configured enables the send control.
- [x] A reply that streamed text keeps it when the stream ends in a proposal
      refusal error.
- [x] Ctrl-C aborts the serve task and the process exits promptly (immediately,
      or within one second).

## Tests

- Frontend regression tests for the send-control enable/disable transitions and
  the keep/drop decision of the reply error path.

## Gotchas

- The send control is derived state; it must be recomputed whenever its inputs
  (composer text, open document, AI config, stream state) change, not only when
  the panel re-renders.
- A reply that ends in a refused proposal streams its text first and only then
  fails, so the error path must treat "streamed text present" as keep-the-answer.
- Graceful shutdown waits for open connections; with a browser-held SSE stream
  the stop can hang, so the stop path aborts the serve task.

## Notes

- Found during the ticket 0061 manual release checklist QA pass; fixes landed
  with clean final reviews on their fix commits.
- Found during manual QA: `cargo run -- --port 4000` did not stop on Ctrl-C
  without `kill -9`.
