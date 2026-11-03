---
status: done
depends_on: ["0020", "0022", "0023"]
---

# 0025: Create, rename and delete UI

**Epic:** [0003 Document API and editor](../description.md) · **Depends on:** 0020, 0022, 0023 · **Milestone:** M1

## Goal

Wire the sidebar actions so a person can manage features, decisions and tickets
without a terminal, with confirmation before deletion.

## Build

- Per-group "New" buttons. For v1 use `window.prompt` for the title (epic,
  feature, decision, ticket); ticket also asks for the epic and optionally a
  feature until 0052 replaces it with selectors.
- Row actions: Rename, Delete. Rename uses `window.prompt` prefilled with the
  title/slug; show the server error on failure.
- Delete uses `window.confirm` and includes the path. A non-empty epic is
  refused by the server; show the ticket count and tell the person to delete or
  move those tickets first. Deleting a feature reports how many tickets still
  name it.
- Core documents have no delete action and no rename action.
- After any mutation: `refreshTree()`, select the created/renamed document,
  reload it, and clear stale editor state.
- Disable buttons while a request is in flight; show errors in the global
  banner.

## Already in place

- Mutation routes, sidebar, editor.

## Acceptance criteria

- [x] Creating each kind adds it to the tree and opens it.
- [x] Epic creation asks only for a title and starts as `planned`.
- [x] Ticket creation asks for an epic and optionally a feature.
- [x] Duplicate name shows the server message; no second file is created.
- [x] Rename keeps content and selection follows the new path.
- [x] Delete asks for confirmation; cancel leaves everything.
- [x] Deleting the open document clears the editor.
- [x] Core files expose edit but not rename/delete.
- [x] Empty or whitespace-only titles never reach the server.

## Tests

- **Manual:** all acceptance cases against a scaffolded temp project.
- **Manual:** delete an epic with two tickets; the request is refused, the count
  is shown, and all files remain.
- **Manual:** delete an empty epic; the directory disappears.
- **Manual:** rename an epic and confirm its number and ticket paths move with
  it.
- **Manual:** cancel each prompt/dialog; no request is sent.
- **Manual:** double-click Create; only one file appears.

## Gotchas

- `window.prompt` cannot show validation inline, but it is acceptable for v1.
  Trim input and reject empty before calling the API.
- After rename, the old path's hash is invalid; always reload from the returned
  path.
- Deleting an epic is refused while it has tickets; say so in the confirmation
  and show the server's count.
- Renaming an epic keeps its number and moves its `tickets/` directory with it;
  ticket relative links to `../description.md` still work.
- Encode action URLs with `docUrl`; a raw path with unusual characters breaks
  the wildcard route.
