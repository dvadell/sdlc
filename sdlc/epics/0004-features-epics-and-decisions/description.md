---
status: planned
---

# 0004: Features, epics and decisions

*Tickets 0028–0032 · Milestone M2*

## Outcome

Features become first-class: create, rename, delete and change status, with the
PRD feature index kept in sync automatically. Epics are numbered directories,
each with a `description.md` status and a `tickets/` folder. Decisions are
numbered ADRs with `proposed | accepted | superseded` status and an optional
`supersedes` link.

## Why this order

The index writer is pure and testable before it is wired into writes. Wiring
comes next, then the UI controls that exercise it. Epics reuse the create,
numbering and status paths built for decisions and features. Decisions reuse the
create and numbering work from the store; only their metadata and UI are new.

## Tickets

| # | Title | Depends on |
|---|-------|------------|
| [0028](tickets/0028-feature-index.md) | PRD feature index generation | 0012, 0013 |
| [0029](tickets/0029-index-refresh.md) | Refresh feature index on feature changes | 0019, 0028 |
| [0030](tickets/0030-feature-status-ui.md) | Feature and epic status UI | 0022, 0023, 0029 |
| [0031](tickets/0031-decision-metadata.md) | Decision metadata and numbering | 0012, 0015 |
| [0032](tickets/0032-decision-ui.md) | Decision UI | 0022, 0023, 0031 |

## Exit criteria

- Creating, renaming, deleting or restatusing a feature updates exactly the
  marked region of `sdlc/PRD.md`.
- A missing marker or missing PRD produces a visible warning but does not lose
  the feature change.
- Decisions get sequential numbers and valid status metadata.
- Epics get sequential numbers, default to `planned`, and can be renamed,
  restatussed or deleted; deleting a non-empty epic is refused with a ticket
  count, and renaming moves its tickets with it.
- Manual checks show feature and epic badges and decision statuses in the
  sidebar.

## Risks

- Rewriting the PRD can conflict with an open editor or a manual edit in the
  same region; 0029 defines the warning behaviour.
- Feature rename leaves ticket `feature:` values stale. This epic does not fix
  that; 0048 exposes `feature_orphaned`.
- Epic deletion is refused while tickets remain. There is no dangling `epic:`
  value to clean up because membership is the path, but a manually broken
  directory can still lack `description.md`; 0053 shows those as unassigned.
- The PRD in this repository is `sdlc/PRD.md` and has no index markers. Do not
  point tests at it; use temp projects.
