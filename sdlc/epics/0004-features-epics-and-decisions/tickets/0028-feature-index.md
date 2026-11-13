---
status: done
depends_on: ["0012", "0013"]
---

# 0028: PRD feature index generation

**Epic:** [0004 Features, epics and decisions](../description.md) · **Depends on:** 0012, 0013 · **Milestone:** M2

## Goal

Generate the one-line-per-feature index deterministically and splice it into
the marked region of the PRD without touching anything else.

## Build

- `src/store/index.rs`:
  - Marker constants `<!-- sdlc:feature-index:start -->` and
    `<!-- sdlc:feature-index:end -->` exported for scaffold and tests.
  - `render(features: &[DocEntry]) -> String`, one line per feature:
    `- [Title](features/slug.md) — shipped`
    Status is the front matter value, or `planned` when absent.
  - `rewrite(prd: &str, entries) -> RewriteOutcome` with
    `Updated(String)` or `NoMarkers`; preserve text before and after the
    region byte-for-byte.
  - Sort by slug for stability, not by title.
  - Detect the PRD's line ending and use it for generated lines.
- Escape `[` and `]` in titles; strip newlines. Slugs are already path-safe.

## Already in place

- Front matter and tree parsing (0012, 0013).

## Acceptance criteria

- [ ] Empty feature list yields an empty region, not a stale line.
- [ ] One line per feature with link relative to `sdlc/PRD.md` and status.
- [ ] Missing status renders `planned`.
- [ ] Title brackets and backslashes cannot break the link.
- [ ] Rewriting an already-correct PRD is a no-op (idempotent).
- [ ] Missing markers returns `NoMarkers` and leaves the input unchanged.
- [ ] CRLF PRDs keep CRLF; text outside the region is untouched.

## Tests

- **Unit:** zero, one and several features; statuses; title escaping.
- **Unit:** idempotence (rewrite twice).
- **Unit:** markers at the start/end of file, markers with surrounding spaces,
  duplicate start markers (use the first, warn).
- **Unit:** missing end marker, missing both.
- **Unit:** CRLF preservation and no trailing newline at EOF.

## Gotchas

- Do not renumber or reorder existing lines by hand-editingaround them; the
  region is owned by sdlc.
- A title containing `]` must become `\]` or the Markdown link breaks.
- Keep the writer pure; file IO belongs to 0029.
- The PRD's own heading structure may place the region under "Feature index";
  do not require that heading.
