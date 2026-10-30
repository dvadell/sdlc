---
status: todo
depends_on: ["0020", "0025", "0048"]
---

# 0052: Ticket create/edit/delete UI

**Epic:** [0007 Ticket generation](../description.md) · **Depends on:** 0020, 0025, 0048 · **Milestone:** M5

## Goal

Let the person manage tickets by hand in the right epic, with an optional
feature link when the ticket came from a feature spec, instead of typing
references into a generic prompt.

## Build

- Create-ticket dialog: title, epic select (existing epics; required), optional
  feature select, initial status `todo`. The selected epic decides the target
  directory; the ticket file is written to
  `sdlc/epics/<epic>/tickets/NNNN-slug.md`. Reject submit when no epic exists,
  with a link to create one.
- Replace the generic ticket prompt from 0025 with this dialog.
- When a ticket is open, show a status select in the toolbar
  (`todo | in-progress | done`), a read-only epic link derived from the path
  that navigates to the epic, and an optional feature link.
- Delete uses confirmation and mentions that dependencies in other tickets are
  not updated. Deleting a ticket removes its file; it does not affect its epic.
- After create, open the new ticket, refresh the tree and keep the current epic
  filter (0053).
- Editing the `feature` line in the textarea is allowed, but validate it on save
  and show a warning when it does not resolve. There is no `epic` line to edit;
  epic membership changes only by moving the file, which v1 does not expose.

## Already in place

- Ticket creation under an epic, editor, sidebar actions, metadata.

## Acceptance criteria

- [ ] Create asks for an epic and writes the ticket under that epic's
  `tickets/` directory.
- [ ] The created front matter has no `epic` key.
- [ ] A feature is optional and, when chosen, produces `feature: <slug>`.
- [ ] No epic in the project disables creation with an explanation.
- [ ] Status control updates front matter and saves.
- [ ] Epic and feature links open the corresponding documents.
- [ ] Delete confirmation warns about dependencies.
- [ ] A malformed directory without `description.md` puts its tickets in the
      Unassigned group; an unknown feature shows a warning but keeps the epic
      group.
- [ ] Ticket numbers are never editable.

## Tests

- **Manual:** create a ticket for each of two epics; check the paths and
  grouping.
- **Manual:** create one with a feature and one without; both keep their epic.
- **Manual:** change status through the toolbar; reload and confirm.
- **Manual:** move a ticket file into an epic directory with no `description.md`
  by hand; reload, see the warning and the Unassigned group.
- **Manual:** delete a ticket; the tree and editor update, and the epic remains.
- **Integration:** creation with an unknown epic is rejected by the store
  (0015); the UI surfaces the message.
- **Manual:** rename the parent epic; the open ticket's epic link still resolves.

## Gotchas

- The epic select must refresh after an epic is created in another tab; reuse
  the tree refresh path rather than caching forever.
- `depends_on` is not editable in the UI v1; it stays in the textarea.
- The H1 number comes from the file name; do not offer a field for it.
- Never write an `epic:` key or offer an epic text field. The path is the only
  binding, and two sources of truth will drift.
- Keep ticket cards compact; the sidebar is narrow.
