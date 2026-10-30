---
status: todo
depends_on: ["0023", "0043", "0044"]
---

# 0045: Diff view UI

**Epic:** [0006 Proposals and prompts](../description.md) · **Depends on:** 0023, 0043, 0044 · **Milestone:** M4

## Goal

Replace the editor with a reviewable diff when a proposal arrives, and accept
or reject the whole thing.

## Build

- `frontend/js/diff.js`:
  - `showProposal(proposal)`: one section per file with path and action
    (`create`, `modify`, `delete`).
  - Color lines by prefix, but treat `+++`/`---` headers and `@@` hunks as
    context, not add/delete (the snippet in the conventions is too naive).
  - Buttons **Accept all** and **Reject**; disable both while a request is in
    flight.
  - On accept, call the route; on success refresh tree, reload any open affected
    document, clear the proposal and return to the editor.
  - On conflict, keep the diff visible with "Files changed since this proposal
    was made. Ask the AI again." and offer only Reject.
  - On partial apply, list exactly which files were written and which failed.
  - On reject, discard and restore the editor without losing unsaved text.
- Preserve `state.current` content while the diff is shown; do not overwrite
  the textarea buffer.
- Show proposal warnings above the diff.
- Add an indicator in the editor when a proposal is pending, so the person does
  not lose track of it.
- The diff is plain text; insert with `textContent`.

## Already in place

- Proposal routes and chat proposal event; editor.

## Acceptance criteria

- [ ] A proposal switches the main area to the diff and shows every file.
- [ ] Added and removed lines are visually distinct; headers are not.
- [ ] Accept writes files and returns to the editor with fresh content.
- [ ] Reject leaves disk unchanged and restores the editor.
- [ ] Conflict shows the explanation and no accept button.
- [ ] Partial apply lists written and failed files.
- [ ] Unsaved editor text survives both accept and reject.
- [ ] A restarted server (unknown id) shows a clear error.

## Tests

- **Manual:** proposal with one modify; accept; inspect the file and git diff.
- **Manual:** proposal with create + delete; reject; nothing changed.
- **Manual:** edit a proposed file externally between proposal and accept;
  conflict path.
- **Manual:** long diff scrolls; buttons stay visible.
- **Manual:** XSS text inside a diff is shown literally.

## Gotchas

- Do not use `innerHTML` for diff lines; a line can contain HTML from the model.
- The editor may be dirty when the proposal arrives; warn before replacing the
  view or keep the buffer and restore silently. Pick one and test.
- After accept, some proposed files may not be in the tree (deleted); refresh
  from the server rather than patching.
- A proposal has no per-hunk controls in v1; do not add checkboxes.
