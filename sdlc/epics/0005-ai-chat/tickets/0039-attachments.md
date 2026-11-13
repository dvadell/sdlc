---
status: done
depends_on: ["0018", "0038"]
---

# 0039: Document attachments

**Epic:** [0005 AI chat](../description.md) · **Depends on:** 0018, 0038 · **Milestone:** M3

## Goal

Let the person add other documents to the conversation context, while always
sending the open document.

## Build

- Attachment control in the chat panel: a button showing a checklist of all tree
  entries except the open document, plus removable chips for current
  attachments.
- `state.chat.attachments: string[]` (paths). Session only.
- Every chat request includes `open_doc` from `state.current.path` (if one is
  open) and the attachment paths.
- The panel shows the open document as a fixed, non-removable chip so it is
  never invisible that it is included.
- On send, drop paths that no longer exist in `state.tree` and warn in the
  bubble.
- Avoid duplicates; attaching the open document through the picker is a no-op.
- Keep the list across document switches; do not clear it on save or tree
  refresh.

## Already in place

- Chat panel, tree, chat route that accepts and loads paths.

## Acceptance criteria

- [ ] Attachments appear as chips and can be removed.
- [ ] The request always contains the open document plus attachments.
- [ ] A deleted attachment is skipped with a visible note, not a 400.
- [ ] No duplicate path is sent.
- [ ] Switching documents updates the fixed chip but keeps other attachments.
- [ ] Attachments reset on page reload.
- [ ] Paths are exact `sdlc/...` or `AGENTS.md` values from the tree.

## Tests

- **Integration (wiremock):** inspect the request body and assert the system
  context contains every attached document, open first.
- **Integration:** attach a path that no longer exists; request succeeds and
  the missing one is absent.
- **Manual:** attach three documents, send, check chips and request payload in
  DevTools.
- **Manual:** remove one chip mid-conversation; the next request omits it.

## Gotchas

- Do not read document contents in the browser and send them; the server reads
  from disk so the context cannot drift from the tree.
- The open document may be a ticket or decision; all valid document kinds are
  allowed.
- Attaching the whole tree is allowed in v1; context size is not managed.
- Keep the picker keyboard-accessible; a plain list of checkboxes is enough.
