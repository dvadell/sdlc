---
status: todo
depends_on: ["0018", "0021"]
---

# 0022: Sidebar document tree

**Epic:** [0003 Document API and editor](../description.md) · **Depends on:** 0018, 0021 · **Milestone:** M1

## Goal

Render the document tree grouped by type, show feature status, and select a
document when clicked.

## Build

- `frontend/js/sidebar.js` with `renderSidebar()` that rebuilds the panel with
  `replaceChildren()`.
- Groups in order: Core, Epics, Features, Decisions, Tickets. Hide empty
  groups.
- Each entry: title, optional status badge, `title` attribute with the path.
  Epics, decisions and tickets show their number. Feature and epic statuses use
  a small text badge, not color alone. Epic rows show their ticket count when
  the tree exposes it.
- Click selects the document: set `state.current.path`, highlight the row, and
  call the editor's `loadDocument(path)` hook (stub until 0023).
- `refreshTree()` fetches `/api/tree` and re-renders, preserving the selected
  path when it still exists.
- Use event delegation on the sidebar container; no inline handlers.

## Already in place

- Shell, API module, tree route.

## Acceptance criteria

- [ ] All groups render in order with correct counts.
- [ ] Epic and feature status show their allowed values; absent/invalid status
      shows no badge.
- [ ] Tickets show their number; epics show their ticket count.
- [ ] The selected row is visibly active after a re-render.
- [ ] A stale selection (deleted file) is cleared.
- [ ] Titles are inserted with `textContent`; a title containing `<b>` is shown
  literally.
- [ ] Empty groups are hidden.

## Tests

- **Manual:** fixture project with two epics, two features, one decision, one
  ticket; verify grouping, epic counts and badges.
- **Manual:** click each kind; the main area identifies the selected path.
- **Manual:** edit front matter status, refresh tree, badge updates.
- **Manual:** delete the selected file via the API and refresh; no stale row.
- **Manual:** a very long title truncates with ellipsis and keeps a tooltip.

## Gotchas

- Do not refetch the document on every render; selection triggers exactly one
  load.
- Re-rendering the sidebar must not steal focus from the editor.
- Feature status values are case-sensitive; unknown values are not an error.
- Tickets will be grouped by epic in 0053; keep the render function easy to
  split.
