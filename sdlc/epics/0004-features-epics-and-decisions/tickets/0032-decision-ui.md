---
status: done
depends_on: ["0022", "0023", "0031"]
---

# 0032: Decision UI

**Epic:** [0004 Features, epics and decisions](../description.md) · **Depends on:** 0022, 0023, 0031 · **Milestone:** M2

## Goal

Create and maintain ADRs from the browser with the number, status and
supersedes link handled for the person.

## Build

- Replace the generic decision prompt from 0025 with a small dialog:
  title, status select, optional supersedes select populated from existing
  decisions (exclude the new one).
- Keep the create button in the Decisions group; default status `proposed`.
- When a decision is open, show a status select in the toolbar (same pattern as
  features in 0030) and a read-only "Supersedes: NNNN" line if present.
- Sidebar decision rows show the number and a status badge.
- The dialog may use native `<dialog>`; keep it in `sidebar.js` or a tiny
  `dialogs.js`. No framework.
- Show server warnings (missing supersedes target) in the banner and still open
  the created file.

## Already in place

- Decision create/validation, editor, sidebar, index unrelated.

## Acceptance criteria

- [ ] Create with each status and an optional supersedes target.
- [ ] The new file opens and the number is assigned by the server.
- [ ] The supersedes select lists existing decisions only.
- [ ] Changing status in the toolbar saves and updates the badge.
- [ ] Sidebar shows decision numbers in numeric order.
- [ ] Deleting a decision asks for confirmation and clears the editor if open.
- [ ] The number cannot be edited from the UI.

## Tests

- **Manual:** create three decisions; numbers are 0001–0003.
- **Manual:** create one that supersedes 0001; front matter and body look
  right; 0001 is unchanged.
- **Manual:** change status to accepted; reload and confirm.
- **Manual:** open a decision with malformed front matter; it still edits.
- **Manual:** delete a decision and confirm the list renumbers nothing.

## Gotchas

- The H1 is `# NNNN: Title`; if the person edits the title in the textarea, the
  file name does not change. Rename remains a separate action.
- Native `<dialog>` needs a fallback focus trap or at least Escape handling;
  test keyboard use.
- Do not auto-mark the superseded decision; the ticket explicitly keeps that
  manual.
- A supersedes target that was deleted makes the select value missing; show it
  as "NNNN (missing)" rather than silently dropping it.
