---
status: done
depends_on: ["0019", "0021"]
---

# 0023: Editor load, save and dirty state

**Epic:** [0003 Document API and editor](../description.md) · **Depends on:** 0019, 0021 · **Milestone:** M1

## Goal

Edit a document in a textarea, track unsaved changes, save with Ctrl/Cmd+S, and
update the stored hash only from the server response.

## Build

- `frontend/js/editor.js`:
  - `loadDocument(path)`: if dirty, ask before discarding; fetch, store
    `{ path, content, savedContent, hash }`, render textarea.
  - `save()`: no-op if clean or already saving; PUT with `expected_hash`;
    on success set `savedContent`, `hash`, clear dirty; on failure keep dirty
    and show the error.
  - Dirty state = textarea value !== `savedContent`; indicator in the header or
    title (`• filename`).
  - Ctrl/Cmd+S saves globally and prevents the browser dialog; Ctrl/Cmd+Enter
    is not bound.
  - `beforeunload` warns when dirty.
- Preserve the file's line endings: detect CRLF on load, convert textarea
  output back to CRLF on save. Otherwise every save rewrites the file.
- Disable the save button while a request is in flight; ignore a second
  submit.

## Already in place

- Save route and shell/state/API.

## Acceptance criteria

- [x] Load, edit, save, reload shows the change.
- [x] Dirty marker appears on first keystroke and clears only after success.
- [x] Ctrl/Cmd+S saves; the browser save dialog does not open.
- [x] A failed save keeps the text and shows the error.
- [x] Switching documents with unsaved changes asks first.
- [x] A CRLF document stays CRLF after save; LF stays LF.
- [x] Saving a document that changed underneath returns 409 (handled in 0027).

## Tests

- **Manual:** the seven acceptance cases; use a temp project with LF and CRLF
  files.
- **Manual:** type while the save is in flight; the textarea remains editable
  and a second save is ignored.
- **Manual:** close the tab while dirty; the browser warns.
- **Manual:** save a large (500 KB) document; no visible stall.

## Gotchas

- The textarea normalizes line endings to `\n`; without the conversion above a
  save produces a whole-file diff in git.
- Update `state.current.hash` only from `{hash}` in the response. Optimistic
  updates cause avoidable conflicts.
- If the person switches documents during a save, the response belongs to the
  old path; compare paths before applying it.
- Do not autosave in v1; it multiplies conflict cases.
