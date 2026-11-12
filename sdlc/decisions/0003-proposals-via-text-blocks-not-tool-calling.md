---
status: accepted
---

# 0003: Proposals via text blocks, not tool calling

## Context

The AI chat must turn a reply into reviewable file changes without giving the
model tools (see architecture sections 5 and 3.5).

## Decision

Use plain-text proposal blocks in the reply instead of tool calling.

## Consequences

- The integration stays compatible with any OpenAI-compatible server.
- The output format is plain text, so the prompt block convention stays simple.
- Malformed or unterminated blocks are skipped with a warning, not fatal.
