---
status: accepted
---

# 0005: Stateless chat endpoint

## Context

The chat has to answer without keeping history the server owns (see architecture
section 3.4).

## Decision

Keep the chat endpoint stateless; the browser holds the conversation and sends
it with every request.

## Consequences

- No server-side history to persist, leak or lose.
- Context size is not managed; the open document plus attachments go with each
  request.
