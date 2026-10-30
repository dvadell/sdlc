---
status: planned
---

# 0005: AI chat

*Tickets 0033–0039 · Milestone M3*

## Outcome

A streaming chat panel that talks to any OpenAI-compatible endpoint. The model
sees the open document plus explicitly attached documents, nothing else, and
has no tools. When AI is not configured, editing still works and the panel
explains exactly which variables to set.

## Why this order

The upstream client and its stream parser are independent of HTTP and can be
tested against `wiremock`. The prompt builder is pure. The route glues them
together, and only then does the browser reader and panel exist. Proposals are
deliberately out of this epic: the model can talk, not write.

## Tickets

| # | Title | Depends on |
|---|-------|------------|
| [0033](tickets/0033-upstream-client.md) | OpenAI-compatible client | 0002, 0003 |
| [0034](tickets/0034-stream-parser.md) | Upstream streaming parser | 0033 |
| [0035](tickets/0035-prompt-builder.md) | System prompt and context builder | 0010, 0013 |
| [0036](tickets/0036-chat-route.md) | Chat SSE route | 0007, 0034, 0035 |
| [0037](tickets/0037-sse-reader.md) | Frontend SSE reader | 0021 |
| [0038](tickets/0038-chat-panel.md) | Chat panel UI | 0007, 0036, 0037 |
| [0039](tickets/0039-attachments.md) | Document attachments | 0018, 0038 |

## Exit criteria

- With a fake OpenAI server, `POST /api/chat` streams `delta` events and ends
  with `done`.
- Upstream failures and disconnects produce one visible `error` and no hang.
- Unconfigured builds disable the send button and show setup instructions.
- The request contains the open document and attachments, labelled with paths.
- Chat history survives only in the page, never on disk.

## Risks

- Providers vary in SSE details; test chunk splitting and `[DONE]` explicitly.
- Sending document text to a remote host is a privacy boundary; 0058 surfaces
  it, but 0038 must already show the configured host.
- Context can be large; v1 explicitly does not manage it. Do not add truncation
  here.
- No tools and no function calling: do not add those fields to the request.
