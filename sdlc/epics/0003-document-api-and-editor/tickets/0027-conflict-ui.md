---
status: todo
depends_on: ["0023"]
---

# 0027: Save conflict UI

**Epic:** [0003 Document API and editor](../description.md) · **Depends on:** 0023 · **Milestone:** M1

## Goal

When a save returns 409, keep the person's work and let them choose between
reloading the disk version and explicitly overwriting it.

## Build

- In `editor.js`, catch `ApiError` with code `conflict`:
  - Keep the textarea contents and dirty state.
  - Show a non-modal banner: "This file changed on disk since you opened it."
  - Buttons: **Reload from disk** (discard local text after a confirm) and
    **Overwrite with mine** (explicit, destructive).
- Overwrite path: `GET` the document for a fresh hash, then `PUT` the current
  textarea content. If that PUT conflicts again, show the banner again and stop.
- Reload path: replace content/hash, clear dirty, keep the selected doc.
- If the file was deleted externally, the GET fails; offer only "Save as new"
  is out of scope, so show the error and keep the text in the editor.
- Never auto-merge and never silently retry.

## Already in place

- Editor save path and 409 handling at the API level.

## Acceptance criteria

- [ ] A simulated external write makes the next save show the banner.
- [ ] Local text is still present and dirty in both choices.
- [ ] Reload shows the disk version and clears dirty.
- [ ] Overwrite writes the local version and updates the stored hash.
- [ ] A second conflict during overwrite is reported, not looped.
- [ ] No file changes unless the person chooses an action.

## Tests

- **Manual:** edit in the app; modify the file with `vi`; save; banner appears.
- **Manual:** choose Reload; disk content wins.
- **Manual:** choose Overwrite; disk matches the app; git diff shows the
  expected change.
- **Manual:** delete the file externally, then save; message is clear and text
  is not lost.
- **Integration (optional):** the 409 path is already covered by 0019; only
  the UI is manual.

## Gotchas

- The overwrite button is destructive; label it explicitly and do not make it
  the default focus.
- When reloading, refetch rather than trusting `state.current.content`.
- The external-change watcher (0056) will offer a similar choice later; keep
  the banner wording consistent.
- Do not clear the textarea on any error path.
