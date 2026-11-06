---
status: done
depends_on: ["0022", "0023", "0029"]
---

# 0030: Feature and epic status UI

**Epic:** [0004 Features, epics and decisions](../description.md) · **Depends on:** 0022, 0023, 0029 · **Milestone:** M2

## Goal

Let the person set a feature's or epic's status from the editor and see the
sidebar and PRD index follow, using the existing save path.

## Build

- When the open document is a feature or epic, show a status `<select>` in the
  editor toolbar:
  - feature: `planned`, `in-progress`, `shipped`;
  - epic: `planned`, `in-progress`, `done`.
- Changing it edits the front matter in the textarea content (add a block if
  absent), marks dirty, and immediately saves through the normal `save()` path.
- If the save fails, restore the select to the document's status and keep the
  text dirty.
- The create flow from 0025 already covers features and epics; add a short hint
  that a new one starts as `planned`.
- Show the non-empty-epic refusal with its ticket count after a delete attempt,
  and the orphan warning after deleting a feature (both returned by the API).
- Refresh the tree after save so the badge (and, for features, the PRD index)
  reflect the new status.

## Already in place

- Editor, sidebar, feature create/delete, index refresh.

## Acceptance criteria

- [x] Selecting a status updates the front matter and saves.
- [x] The sidebar badge and PRD line update after a feature save.
- [x] An epic status change updates only the epic badge.
- [x] A feature or epic without front matter gains a valid block.
- [x] A failed save does not lie about the status.
- [x] Other document kinds show no status control.
- [x] Deleting a non-empty epic shows the refusal and ticket count; no ticket
  becomes unassigned.

## Tests

- **Manual:** each feature status value; inspect the file and the PRD.
- **Manual:** each epic status value; inspect the file and the sidebar.
- **Manual:** remove front matter, set status, check the inserted block.
- **Manual:** make the file read-only externally, change status, see the error.
- **Manual:** create an epic and a feature; confirm both start as `planned`.
- **Integration:** the underlying save/index path is covered by 0029.

## Gotchas

- Replacing front matter with a string edit is fragile; parse the block, update
  the one key, and preserve unknown keys and formatting where possible. If that
  is too much, regenerate the whole known block and state the limitation.
- Do not offer a status value outside the allowed set for that kind.
- Saving on every select change can race; disable the control while in flight.
- The PRD index is updated by the server, not by the UI; never patch the PRD
  text locally. Epic status has no PRD index entry.
