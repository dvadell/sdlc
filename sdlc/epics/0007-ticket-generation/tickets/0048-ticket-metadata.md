---
status: done
depends_on: ["0012", "0013"]
---

# 0048: Ticket metadata and epic grouping

**Epic:** [0007 Ticket generation](../description.md) · **Depends on:** 0012, 0013 · **Milestone:** M5

## Goal

Group every ticket by the epic directory that contains it, keep the optional
feature link, flag malformed epic directories, and expose the grouping to the
API and UI.

## Build

- Add a typed conversion from generic front matter:
  ```rust
  pub struct TicketMeta {
      pub feature: Option<String>,
      pub status: Option<TicketStatus>,
      pub depends_on: Vec<u32>,
  }
  ```
  There is deliberately no `epic` field; the path is the source of truth.
- Derive the epic from `sdlc/epics/<epic>/tickets/<ticket>.md`. Resolve it to
  `sdlc/epics/<epic>/description.md`; if that file is missing, mark the ticket
  `orphaned: true` but keep the directory name as `epic` so it can be shown.
- `feature`, when present, is a slug without `.md`, resolved to
  `sdlc/features/<slug>.md`.
- Extend `DocEntry` for tickets with `epic`, `feature`, `depends_on` and the
  derived `orphaned: bool`.
- Add a separate derived `feature_orphaned: bool` when a feature was named but
  no longer resolves. It does not affect epic grouping.
- `tree()` sorts tickets numerically within their directory and does not drop
  orphans.
- Expose orphans in the tree response so 0053 can show an "Unassigned" group
  for manually malformed directories. Normal deletion cannot create one.

## Already in place

- Front matter parsing, tree, and ticket creation under an epic directory.

## Acceptance criteria

- [ ] A valid ticket reports its epic directory, optional feature and status.
- [ ] A ticket whose parent epic has no `description.md` is `orphaned: true`
  but still listed.
- [ ] A ticket's epic cannot disagree with its path because there is no `epic`
  front matter field.
- [ ] Renaming an epic's directory does not orphan its tickets.
- [ ] A ticket with a stale feature is not an orphan by epic, but reports
      `feature_orphaned: true`.
- [ ] `depends_on` is parsed as numbers; invalid entries are ignored with a
      warning.
- [ ] Grouping is deterministic.

## Tests

- **Unit:** valid parent description, missing description, and duplicate
  directory numbers (which should be reported as an invalid layout).
- **Unit:** rename an epic directory with tickets; assert no ticket changes and
  no orphan.
- **Unit:** optional feature present, absent, renamed/deleted.
- **Unit:** `depends_on: ["0004", "0005"]` behavior and malformed entries.
- **Unit:** ordering: `0009` before `0010`.
- **Unit:** one ticket appears exactly once in its epic group.

## Gotchas

- The directory is the epic reference, not a front matter number. Do not
  resurrect `epic: "0001"` or fall back to matching ticket ids.
- The template uses `feature: export`, not `feature: export.md`; document the
  choice and test the resolver.
- Epic deletion is refused while tickets exist (0016). `orphaned` is only for
  manually broken directories, not a normal lifecycle state.
- Do not make `orphaned` part of the stored file; it is derived.
- Keep ticket status separate from epic and feature status; they share the word
  "status" but have different enums.
