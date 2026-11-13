---
status: done
depends_on: ["0019", "0028"]
---

# 0029: Refresh feature index on feature changes

**Epic:** [0004 Features, epics and decisions](../description.md) · **Depends on:** 0019, 0028 · **Milestone:** M2

## Goal

Keep the PRD index correct whenever a feature is created, renamed, deleted,
retitled or changes status, without making the feature operation fail when the
PRD cannot be updated.

## Build

- Add an internal `refresh_feature_index()` to `DocStore`: read all feature
  entries, read `sdlc/PRD.md` if present, call `index::rewrite`, write only when
  the result differs.
- Call it after:
  - `create(Feature, ...)`
  - `rename` where either side is a feature
  - `delete` of a feature
  - `write` of a feature (covers title and status changes)
- Do **not** call it when writing `sdlc/PRD.md` itself.
- If the PRD is missing or markers are missing, log a warning and continue;
  return a warning string in the operation result where the API can expose it.
- Extend `SaveResponse` and mutation responses with optional
  `warnings: [string]`; existing clients ignore it.
- Accept the small race window on the PRD read-modify-write; document it.

## Already in place

- Save/create/rename/delete, index writer, tree.

## Acceptance criteria

- [ ] Create feature → line appears in the marked region.
- [ ] Change `status` via save → line status updates.
- [ ] Rename → link and title update, old line gone.
- [ ] Delete → line removed.
- [ ] Editing a non-feature document never touches the PRD.
- [ ] Missing PRD or markers → feature operation succeeds with a warning.
- [ ] The PRD is not rewritten when its index is already correct.

## Tests

- **Integration:** each of the four operations against a temp project; assert
  the full PRD around the region is unchanged.
- **Integration:** PRD without markers; assert 200 plus warning, feature file
  exists.
- **Integration:** save a feature twice without changes; PRD mtime/hash stable
  (no useless write).
- **Unit:** warning mapping in the response DTO.

## Gotchas

- Writing the PRD changes its hash; an editor holding the old hash will get a
  409 on its next save. That is correct, but 0056 improves the UX.
- Do not take the store write lock across reading every feature; copy the list
  first.
- Index refresh must not recurse into itself when the feature list includes the
  PRD.
- If the feature write succeeds and the PRD write fails, report a warning; never
  claim the feature was not saved.
