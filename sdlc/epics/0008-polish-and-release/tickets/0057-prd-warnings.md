---
status: done
depends_on: ["0002", "0010", "0023"]
---

# 0057: PRD size and smell warnings

**Epic:** [0008 Polish and release](../description.md) · **Depends on:** 0002, 0010, 0023 · **Milestone:** M6

## Goal

Warn, without blocking, when the PRD grows too large or contains content that
belongs in architecture, conventions or a feature spec.

## Build

- Add `SDLC_PRD_MAX_BYTES` to `Config` and `/api/config`; default `12000`,
  positive integer, upper bound `1_000_000`. Document it in `conventions.md`,
  the architecture config table and README.
- Extend the PRD document response with `warnings`:
  - `{ kind: "prd_size", bytes, limit }` when size > limit;
  - `{ kind: "code_block", count }` when fenced code blocks exist;
  - `{ kind: "stack_heading", heading }` when a heading case-insensitively
    contains `dependencies`, `libraries`, `stack`, `tooling` or `code style`.
- Compute only for `sdlc/PRD.md`; other documents get an empty list.
- UI: a dismissible banner above the PRD editor, not a modal. Dismissal is
  session-only. Link to `architecture.md`/`conventions.md` as the likely home.
- Warnings never prevent saving.

## Already in place

- Config, document read, editor.

## Acceptance criteria

- [ ] Below/equal to the limit → no size warning; above → warning with bytes.
- [ ] A fenced code block triggers exactly one warning with a count.
- [ ] Inline code does not count.
- [ ] Matching headings trigger with the heading text.
- [ ] Warnings appear only for the PRD.
- [ ] The UI banner is dismissible and reappears after reload.
- [ ] Saving an over-limit PRD still succeeds.

## Tests

- **Unit:** size boundaries (limit−1, limit, limit+1); bytes not chars
  (multi-byte heading).
- **Unit:** paired, unpaired and indented fences; multiple fences count.
- **Unit:** heading case, `##`/`###`, headings inside code fences ignored.
- **Integration:** `/api/config` exposes the override; doc response includes
  warnings.
- **Manual:** banner on a bloated PRD; dismiss; reload.

## Gotchas

- "Library names" cannot be detected reliably; do not add a brittle dependency
  denylist. Headings plus code fences are the honest heuristics.
- A code block in the PRD's own feature-index example would trigger a warning;
  the starter PRD must avoid fenced examples.
- Warnings are advisory; never block the feature index or a save.
- The config value is bytes, not characters; make the naming explicit.
