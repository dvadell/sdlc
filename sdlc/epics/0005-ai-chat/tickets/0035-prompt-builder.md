---
status: todo
depends_on: ["0010", "0013"]
---

# 0035: System prompt and context builder

**Epic:** [0005 AI chat](../description.md) · **Depends on:** 0010, 0013 · **Milestone:** M3

## Goal

Build the messages sent upstream: a fixed system prompt plus the open document
and attachments, each labelled with its path. No source code, no tools.

## Build

- `src/chat/prompt.rs`:
  - `SYSTEM_PROMPT` constant covering: role (document editor for a software
    project), scope (only the supplied documents), no tools/commands, proposal
    format from architecture section 5, full-file content in `FILE` blocks,
    paths relative to the project root, ask instead of guessing.
  - `build(open: &Document, attachments: &[Document], history: &[Message])
    -> Vec<Message>`.
  - Context is one system message (rules + documents) or a separate context
    message; pick what works with single-system-message servers and document
    it.
  - Each document appears as a delimiter line with its path, then its exact
    content: `=== DOC: sdlc/features/export.md ===`.
  - Open document first, then attachments sorted by path; de-duplicate.
- `Message { role: user | assistant, content: String }`; reject a client-supplied
  `system` role at the route.
- History is appended after the context, unchanged.

## Already in place

- Store reads, tree paths, client request shape.

## Acceptance criteria

- [ ] The system prompt states the proposal format and the no-tools rule.
- [ ] The open document is always present and first.
- [ ] Attachments are labelled, ordered and deduplicated.
- [ ] A missing attachment is skipped; the builder does not fail.
- [ ] No absolute paths appear in the prompt.
- [ ] History order is preserved and its content is not rewritten.

## Tests

- **Unit:** prompt contains the marker rules and both delimiters.
- **Unit:** open doc + two attachments; assert order and labels.
- **Unit:** duplicate path appears once; open doc repeated as attachment is
  ignored.
- **Unit:** empty history; whitespace-only messages; missing doc skipped.
- **Unit:** a document containing `=== FILE:` text is included verbatim (the
  parser only trusts the model's reply, not the context).

## Gotchas

- Do not try to manage context size; that is an explicit non-goal. Do not add
  truncation "just in case".
- Use the document path as the label, never the absolute path.
- Keep delimiters unlikely but not impossible; a document may contain them.
  That is a prompt-injection surface, accepted for a local single-user tool.
- Reference the format from one constant so the parser and prompt cannot drift
  apart silently.
