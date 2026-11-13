---
status: done
depends_on: ["0038", "0061"]
---

# 0066: New ticket creation fails with bad request

**Epic:** [0009 Prerelease bugs](../description.md) · **Depends on:** 0038 · **Milestone:** M6

## Goal

Creating a new ticket through the "New ticket" dialog must send a valid create
body, so filing a ticket works instead of answering `bad request`.

## Build

- Resolve the create title from the ticket-create outcome instead of letting the
  generic prompt fallback override it, so a ticket create sends the dialog title.

## Acceptance criteria

- [x] Clicking "New ticket" and submitting with a title and an epic creates the
      ticket instead of failing with a bad request.

## Tests

- Frontend regression tests for the create title resolution of tickets and
  decisions and of the prompt-based kinds.

## Gotchas

- Tickets create through the kind-specific ticket dialog, so their title never
  comes from the generic prompt; the create flow must not fall back to it.

## Notes

- Found during manual QA: the create answered `The request failed (bad_request):
  bad request`, and the server logged `WARN sdlc::routes::docs: request body
  rejected status=422 Unprocessable Entity`.
