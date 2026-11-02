---
status: done
depends_on: ["0015"]
---

# 0016: Rename and delete documents

**Epic:** [0002 Document store](../description.md) · **Depends on:** 0015 · **Milestone:** M1

## Goal

Support the lifecycle actions the sidebar needs, with the fixed core documents
protected from deletion.

## Build

- `DocStore::rename(from: &DocPath, to: &DocPath) -> Result<DocPath, StoreError>`:
  - Both must be the same `DocKind`; core documents cannot be renamed.
  - Epics, decisions and tickets must keep their numeric prefix; only the slug
    may change. Reject a changed number.
  - Renaming an epic renames its whole directory (`sdlc/epics/<epic>/`), so its
    `tickets/` move with it and no ticket front matter changes. Renaming a
    ticket moves it only within its current epic.
  - Reject if the destination exists.
  - Use `fs::rename`; no read/write round trip.
- `DocStore::delete(path) -> Result<DeleteReport, StoreError>`:
  - Core documents are `NotAllowed`.
  - Epics, features, decisions and tickets may be deleted.
  - An epic may be deleted only when its `tickets/` directory contains no
    `.md` files; otherwise return `Conflict` with the ticket count and delete
    nothing. There is no automatic recursive delete and no unassigned ticket
    state created by the product.
  - `DeleteReport { orphaned_tickets: usize }`: for a feature, count tickets
    whose `feature` slug matches. Do not delete or rewrite those tickets.
- Resolve both paths before touching disk; never follow a symlink out.

## Already in place

- Create, layout, read/write, front matter (0015 and earlier).

## Acceptance criteria

- [x] Renaming a feature changes its file name and keeps content.
- [x] Renaming an epic changes its directory name and leaves its ticket paths
  inside the new directory; the returned path is the new `description.md`.
- [x] Core documents cannot be renamed or deleted; both return `not_allowed`.
- [x] An epic/decision/ticket rename cannot change its number; changing it is
  an error.
- [x] Cross-kind renames are rejected.
- [x] Destination collision is rejected without overwriting.
- [x] Deleting `PRD.md`, `architecture.md`, `conventions.md` or `AGENTS.md`
  returns `not_allowed`.
- [x] Deleting a non-empty epic returns `conflict` and leaves the directory and
  all tickets untouched.
- [x] Deleting an empty epic removes the directory.
- [x] Deleting a feature reports how many tickets reference it.
- [x] Deleting a missing file returns `NotFound`.

## Tests

- **Unit:** rename success, number change, cross-kind, collision, missing.
- **Unit:** case-only rename on a case-insensitive filesystem (document the
  behaviour if skipped).
- **Unit:** rename an epic with tickets; assert the tickets moved and still
  parse.
- **Unit:** delete each kind; core protection; non-empty epic conflict; empty
  epic removal; feature orphan count.
- **Security:** rename/delete paths with traversal and symlink escape.

## Gotchas

- An epic directory and its `tickets/` are one unit. Rename the directory, not
  just `description.md`, or every ticket becomes unassigned.
- Deleting a non-empty epic is deliberately refused. Do not implement a
  recursive delete as a convenience; data loss in a planning tool is worse than
  a second action.
- Renaming a feature orphans tickets by feature; renaming an epic does not,
  because tickets move with their directory.
- The PRD feature index still shows the old name until 0029 refreshes it; do not
  duplicate that logic here.
- A rename and a delete both need the watcher suppression added in 0055; note
  them as write operations there.
- On Windows, renaming an open file or directory can fail; surface the IO error,
  do not retry forever.
