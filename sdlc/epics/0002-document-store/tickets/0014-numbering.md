---
status: todo
depends_on: ["0008"]
---

# 0014: Sequential number allocation

**Epic:** [0002 Document store](../description.md) · **Depends on:** 0008 · **Milestone:** M1

## Goal

Give epics, decisions and tickets collision-free four-digit numbers, even when
two creates race.

## Build

- `src/store/numbering.rs`:
  - `next_epic_number() -> Result<u32, StoreError>`: scan
    `sdlc/epics/NNNN-*/description.md`, take the maximum, return `max + 1`.
  - `next_ticket_number() -> Result<u32, StoreError>`: scan
    `sdlc/epics/*/tickets/NNNN-*.md` across every epic, take the global maximum,
    return `max + 1`.
  - `next_decision_number() -> Result<u32, StoreError>`: scan
    `sdlc/decisions/NNNN-*.md`.
  - `format_epic_name(n, slug) -> String` → `0007-slug` (a directory).
  - `format_ticket_name(n, slug) -> String` → `0007-slug.md`.
  - A create helper that allocates and calls `create_new`; on `AlreadyExists`
    it re-scans and retries, up to a small bound.
- Epic, decision and ticket sequences are independent, but ticket numbers are
  global across all epics.
- Error above `9999` with a clear message; do not silently use five digits.

## Already in place

- `DocPath`/`DocKind` (0008); write primitives (0011, used by 0015).

## Acceptance criteria

- [ ] Empty epic directory set → `0001`; empty ticket set → `0001`.
- [ ] Existing epic `0001`, `0003` → `0004`.
- [ ] Existing tickets `0001` and `0003` in different epics → `0004`.
- [ ] Files and directories without a valid prefix are ignored.
- [ ] Two concurrent creates get different numbers.
- [ ] `9999` is the last valid number; the next attempt fails cleanly.
- [ ] The number is zero-padded to four digits.

## Tests

- **Unit:** empty, gap, contiguous, malformed names, non-Markdown files.
- **Unit:** numeric ordering: `0009` sorts before `0010`.
- **Unit:** epic directory scan yields its own sequence; ticket scan spans all
  epic `tickets/` directories and yields the global next number.
- **Concurrency:** spawn two creators against one temp dir; assert distinct
  names and no overwrite.
- **Unit:** boundary at `9999`.

## Gotchas

- Scanning and creating is not atomic; the retry on `create_new` is what makes
  it safe. Do not reserve a number without creating the file.
- Gaps are normal after a delete; never renumber existing files.
- Parse the prefix as a number, not lexically, when finding the maximum.
- Keep allocation inside the store; the AI must never choose a ticket number
  (0050).
