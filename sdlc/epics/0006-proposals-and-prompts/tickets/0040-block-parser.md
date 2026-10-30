---
status: todo
depends_on: ["0008"]
---

# 0040: FILE and DELETE block parser

**Epic:** [0006 Proposals and prompts](../description.md) · **Depends on:** 0008 · **Milestone:** M4

## Goal

Extract proposed file writes and deletes from a model reply, ignoring malformed
blocks with a warning instead of failing the conversation.

## Build

- `src/chat/blocks.rs`:
  ```rust
  pub enum Block { Write { path: String, content: String }, Delete { path: String } }
  pub struct Parsed { pub blocks: Vec<Block>, pub warnings: Vec<String> }
  ```
- Marker grammar, one per line:
  - `=== FILE: <path> ===` … `=== END FILE ===`
  - `=== DELETE: <path> ===`
- Accept optional leading/trailing whitespace on marker lines and markers
  inside or outside code fences (models often wrap them). Document the choice.
- Content is everything between the markers, preserved byte-for-byte except the
  marker lines. Keep a final newline before `END FILE` if present.
- Warnings, skip the block: missing end marker, empty path, path with newline,
  nested `FILE` before `END`, duplicate path (keep the last with a warning).
- Text outside blocks is ignored by this parser; the chat already streamed it.

## Already in place

- `DocPath` (validation happens in 0041, not here).

## Acceptance criteria

- [ ] One write block yields path and exact content.
- [ ] Multiple writes, a delete, and mixed order parse.
- [ ] Unterminated block is skipped with a warning.
- [ ] Empty content is a valid write.
- [ ] Duplicate paths produce one block and one warning.
- [ ] Marker-like text in ordinary prose without an end marker is skipped.
- [ ] CRLF input yields LF content (normalize markers, not user text? decide and
  test).

## Tests

- **Unit:** every case above.
- **Unit:** content containing `=== END FILE ===` on a non-marker line inside a
  code fence.
- **Unit:** markers wrapped in a fenced code block; assert chosen behaviour.
- **Unit:** path with spaces is returned trimmed; validation rejects it later.
- **Unit:** no blocks → empty vec, no warnings.

## Gotchas

- Do not use a regex with `.*` across the whole reply; scan lines for
  predictable behaviour and linear time.
- The full new content is authoritative; never try to reconstruct it from a
  patch.
- A `DELETE` block has no body; do not consume the following lines.
- Normalize a single trailing newline consistently, or diffs will show a
  spurious blank line on every proposal.
