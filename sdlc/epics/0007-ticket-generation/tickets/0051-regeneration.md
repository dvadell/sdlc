---
status: todo
depends_on: ["0049", "0050"]
---

# 0051: Regenerate tickets against existing files

**Epic:** [0007 Ticket generation](../description.md) · **Depends on:** 0049, 0050 · **Milestone:** M5

## Goal

When tickets already exist for this feature in the selected epic, include them
in the generation context so the diff shows edits instead of duplicating or
silently losing manual work.

## Build

- Extend the generation flow: if the feature has tickets in the selected epic,
  attach their full contents and tell the model:
  - propose `FILE` blocks using the existing numbered paths for changes;
  - use `DELETE` only when a ticket is truly obsolete;
  - keep manual edits unless they contradict the feature;
  - do not renumber existing tickets or move them to another epic directory.
- Do not include tickets from other epics, other features, or malformed epic
  directories.
- The proposal's modify/delete changes get `base_hash` from disk, so accepting
  stale content conflicts rather than overwriting.
- Show a note above the diff: "N existing tickets included".
- No automatic deletion of tickets the model does not mention.

## Already in place

- Generation prompt, numbering, proposal diff and accept.

## Acceptance criteria

- [ ] The request includes exactly the feature's existing tickets in the
      selected epic.
- [ ] Unchanged tickets produce no change (model may omit them).
- [ ] A modified ticket appears as `modify` with a diff.
- [ ] A `DELETE` appears as a delete with the full old content in the diff.
- [ ] Manual edits are visible in the diff and only lost if accepted.
- [ ] Tickets in other epics, other features and malformed epic directories are
      not included.
- [ ] A ticket changed after the proposal causes a conflict at accept.

## Tests

- **Integration (wiremock):** assert the request contains the two existing
  tickets in the selected epic and no others.
- **Integration:** reply modifies one and deletes one; proposal actions are
  correct.
- **Unit:** no existing tickets → same behaviour as 0049.
- **Manual:** generate, hand-edit a ticket, regenerate, confirm the diff shows
  the hand edit.
- **Manual:** accept a regeneration; confirm numbering did not shift.

## Gotchas

- Existing tickets can be numerous; context size is still unmanaged. Let the
  person detach some before sending rather than truncating silently.
- The model may rewrite everything even when asked not to; the diff is the
  safety mechanism, so do not auto-accept.
- Deleting a ticket that another ticket depends on leaves a dangling
  `depends_on`; a warning in the diff is enough for v1.
- Include tickets sorted by number for deterministic prompts.
