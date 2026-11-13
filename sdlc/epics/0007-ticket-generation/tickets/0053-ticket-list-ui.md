---
status: done
depends_on: ["0022", "0030", "0048"]
---

# 0053: Ticket list and status UI

**Epic:** [0007 Ticket generation](../description.md) · **Depends on:** 0022, 0030, 0048 · **Milestone:** M5

## Goal

Make a growing ticket list navigable: group by epic, show status, and surface
unassigned tickets instead of hiding them.

## Build

- In the sidebar, group tickets under their epic:
  `Epics` entries keep their status; nested or adjacent ticket rows show
  number, title, optional feature and status badge.
- Add an **Unassigned** group for `orphaned: true` tickets, with a short hint
  that the parent directory has no `description.md` (normally caused by manual
  edits, not by the UI; epic deletion is refused while tickets exist).
- Optional filter: "Tickets for current epic" toggle; default off. Keep it in
  `state`, session only.
- Show counts per group (e.g. `Epics (3)` and tickets per epic), and hide empty
  groups.
- Order epics by number, tickets within an epic by number; do not nest by
  feature.
- Status badge text is accessible (not color alone).

## Already in place

- Ticket metadata/grouping API, sidebar, status control.

## Acceptance criteria

- [ ] Tickets appear under their epic, sorted by number.
- [ ] Unassigned tickets appear under Unassigned.
- [ ] Status badges reflect front matter; unknown status shows none.
- [ ] The filter shows only tickets for the open epic and is reversible.
- [ ] Selecting a ticket still loads it and highlights it.
- [ ] A newly generated ticket appears after the tree refresh.
- [ ] Renaming an epic keeps its tickets attached; deleting a non-empty epic is
      refused and the tree does not move anything to Unassigned.

## Tests

- **Manual:** fixture with two epics, four tickets and one manually malformed
  epic directory; verify grouping, counts and filter.
- **Manual:** change a ticket status and refresh; badge updates.
- **Manual:** accept a generated ticket proposal; the new rows appear under the
  chosen epic.
- **Manual:** try to delete an epic with tickets; the refusal appears and its
  tickets stay grouped.
- **Manual:** 30 tickets; scrolling and selection remain usable.

## Gotchas

- Nesting every ticket under an epic can make a 60-ticket project long; the
  filter is the escape hatch, not a second data model.
- Do not hide unassigned tickets; they are the visible consequence of a
  malformed on-disk directory.
- Reuse the sort order from the API; do not re-sort by title in the browser.
- Keep the render function from 0022 easy to extend; avoid duplicating row
  markup.
- Feature is secondary metadata here; do not create a second grouping level.
