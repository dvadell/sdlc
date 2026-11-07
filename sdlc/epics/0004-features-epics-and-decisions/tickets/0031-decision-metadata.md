---
status: done
depends_on: ["0012", "0015"]
---

# 0031: Decision metadata and numbering

**Epic:** [0004 Features, epics and decisions](../description.md) · **Depends on:** 0012, 0015 · **Milestone:** M2

## Goal

Make decisions behave as ADRs: sequential numbers, a constrained status, and an
optional `supersedes` pointer that is validated.

## Build

- Extend decision creation to accept `status` (default `proposed`) and
  `supersedes` (optional decision number).
- Decision template already has front matter `status`; ensure the H1 is
  `# NNNN: Title` and the number matches the file name.
- Validation:
  - status must be one of `proposed | accepted | superseded`.
  - `supersedes` must refer to an existing decision file; a missing target is a
    warning on creation, not a hard failure (the target may be added later).
  - A decision cannot supersede itself.
- Do not automatically change the old decision's status in v1; that is a manual
  edit and the UI should say so.
- Expose `supersedes` and status in the tree entry for decisions.

## Already in place

- Decision create/numbering, front matter parsing, tree.

## Acceptance criteria

- [x] New decisions default to `proposed` and get the next number.
- [x] An accepted/superseded decision keeps its number after edit.
- [x] Invalid status is rejected at creation, not written.
- [x] `supersedes` pointing at a missing number produces a warning.
- [x] `supersedes` pointing at itself is rejected.
- [x] Tree entries include status and supersedes.

## Tests

- **Unit:** creation for each status; numbering across a gap.
- **Unit:** self-supersede, missing target, non-numeric target.
- **Unit:** front matter round trip preserves `supersedes`.
- **Unit:** a decision without front matter still parses and lists.

## Gotchas

- `supersedes` is a number without leading zeros in YAML? Decide: store as an
  integer and compare numerically; the file name keeps padding.
- Cycles (`A supersedes B`, `B supersedes A`) are possible. Warn if trivial to
  detect; do not build a graph in v1.
- Deleting a decision that is superseded leaves a dangling pointer; tree
  rendering should tolerate it.
- Status changes are ordinary document saves in this milestone; no dedicated
  endpoint.
