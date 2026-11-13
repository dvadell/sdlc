---
status: todo
depends_on: []
---

# 0068: Sidebar epics section lists all tickets

## Description

The sidebar in the web UI renders the "Epics" section with the correct count of
epics (9), but the section body lists every ticket up to the latest instead of
listing only the epics; each epic should be collapsible and hold its tickets
inside.

## Acceptance criteria

- [ ] The epics section of the sidebar displays only the epics, with each epic
      collapsible and holding its tickets inside.

## Tests

- Frontend regression tests for the epic ticket-list toggle.

## Notes

- Found during manual QA after the sidebar group-collapsible feature: the Epics
  section showed the epic count beside a body that carried the tickets.
