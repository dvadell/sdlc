---
status: planned
---

# 0009: Prerelease bugs

*Tickets 0065 · Milestone M6*

## Outcome

Fix the defects found while exercising the release candidate before calling v1
done: the AI chat send control stayed disabled after a document was opened and
the composer was filled, a streamed AI reply that ended in a proposal refusal
lost its text, and the server did not stop on Ctrl-C while a client held the
file-change SSE stream open.

## Why this order

The release checklist gates the v1 call; every failure it surfaces during the
manual pass becomes a prerelease bug ticket instead of being fixed silently in
the checklist itself.

## Tickets

| # | Title | Depends on |
|---|-------|------------|
| [0065](tickets/0065-prerelease-bug-fixes.md) | Prerelease bug fixes | 0038, 0061 |
| [0066](tickets/0066-new-ticket-create-bad-request.md) | New ticket creation fails with bad request | 0038, 0061 |

## Exit criteria

- The chat send control enables as soon as the composer holds text with an
  open document and an AI backend configured.
- A streamed AI reply that ends in a proposal refusal keeps its text.
- The server stops on Ctrl-C immediately or within one second.

## Risks

- Manual QA surfaced the stale send-control state, the reply-text loss and the
  Ctrl-C shutdown hang; the fixes must keep the derived state in sync with
  composer edits and document selection, keep streamed reply text on refusal,
  and abort the serve task on stop.
