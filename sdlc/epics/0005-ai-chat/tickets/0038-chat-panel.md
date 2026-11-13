---
status: done
depends_on: ["0007", "0036", "0037"]
---

# 0038: Chat panel UI

**Epic:** [0005 AI chat](../description.md) · **Depends on:** 0007, 0036, 0037 · **Milestone:** M3

## Goal

A usable chat beside the editor: send, stream, stop, show errors, and explain
how to configure AI when it is missing.

## Build

- `frontend/js/chat.js`:
  - Message list with role labels; user text via `textContent`; assistant text
    plain while streaming, sanitized Markdown once complete.
  - Input textarea: Enter sends, Shift+Enter inserts a newline; send disabled
    while streaming or empty.
  - Stop button aborts the request; partial text stays in the transcript.
  - `state.chat.messages` only; no localStorage, no server history.
  - Auto-scroll only when the person is already at the bottom.
- Unconfigured state: replace input with instructions naming
  `SDLC_AI_BASE_URL`, `SDLC_AI_MODEL` and optional `SDLC_AI_API_KEY`, plus
  "restart sdlc after setting them".
- Always show the configured AI host from `/api/config` and a one-line notice
  that document content is sent to it.
- Map `error` events, HTTP errors and connection loss to a message bubble; never
  leave the spinner running.
- Add the privacy notice to the panel, not just the README.

## Already in place

- Chat route, SSE reader, config endpoint, shell.

## Acceptance criteria

- [ ] Send with a fake or real local model streams text into one bubble.
- [ ] Enter sends; Shift+Enter adds a line.
- [ ] Stop leaves the partial reply and allows a new message.
- [ ] A 401/500/network failure shows a readable bubble.
- [ ] Unconfigured shows setup instructions and no send control.
- [ ] With no open document, the panel asks the person to select one and
      disables send (the open document is always part of the context).
- [ ] Refresh clears the transcript; nothing is persisted.
- [ ] AI output containing HTML/script is not executed.

## Tests

- **Manual:** each acceptance case against wiremock-driven or a local
  OpenAI-compatible server.
- **Manual:** send a long reply and scroll up mid-stream; auto-scroll pauses.
- **Manual:** press Enter twice quickly; only one request is sent.
- **Manual:** abort, then send again; no stale stream writes into the new
  bubble.
- **Manual:** XSS payload returned by the model.

## Gotchas

- Track the active request id; an aborted stream may still deliver one buffered
  chunk after the next request starts.
- Rendering Markdown for every delta is expensive and unsafe; render plain text
  during streaming, sanitize once at the end.
- Do not store the transcript in `localStorage`, even "just for reload"; that
  violates the non-goal.
- The panel must remain visible when AI is unconfigured; only the input is
  replaced.
- Keep the host display derived from `/api/config` (`ai_host`), not the full
  base URL if it contains credentials.
