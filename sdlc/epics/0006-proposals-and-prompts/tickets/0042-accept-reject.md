---
status: todo
depends_on: ["0011", "0029", "0041"]
---

# 0042: Accept and reject proposals

**Epic:** [0006 Proposals and prompts](../description.md) · **Depends on:** 0011, 0029, 0041 · **Milestone:** M4

## Goal

Apply all changes of a proposal only if every file is still in the state the
proposal saw, and report exactly what happened if a write fails midway.

## Build

- `ProposalService::accept(id, store) -> Result<AcceptReport, ProposalError>`:
  1. Re-check every change: `Modify`/`Delete` hash must equal `base_hash`;
     `Create` must still be missing. Any mismatch → `Conflict`, write nothing.
  2. Apply in block order with atomic writes, using internal store write/delete
     operations that do **not** auto-refresh the feature index (0029); the
     index is a proposal-level concern. Record applied paths.
  3. If a write fails, stop and return `Partial { applied, failed, reason }`;
     do not pretend it was atomic.
  4. After success, refresh the feature index if a feature was written or
     deleted. If the proposal itself contains `sdlc/PRD.md`, skip the automatic
     refresh so the proposed PRD content wins.
  5. Remove the proposal on success.
- `reject(id)` removes it. Unknown id → `NotFound`.
- On conflict, keep the proposal so the person can inspect or reject it; it
  cannot be applied until regenerated.
- Guard against two concurrent accepts of the same id: one wins, the other gets
  `NotFound` or `InProgress`.

## Already in place

- Proposal building, atomic writes, index refresh.

## Acceptance criteria

- [ ] Accept applies creates, modifies and deletes.
- [ ] A stale hash returns conflict and writes nothing.
- [ ] A create whose target appeared meanwhile returns conflict.
- [ ] A delete of a file changed since proposal returns conflict.
- [ ] Midway failure reports applied files and the failing path, without
      rolling back silently, and leaves the proposal available for inspection.
- [ ] Feature changes refresh the PRD index; an explicit PRD change is kept.
- [ ] Reject does not touch disk; accept removes the proposal.
- [ ] A second accept of the same id fails cleanly.

## Tests

- **Unit/integration:** every case above in a temp project.
- **Unit:** partial failure simulated with a read-only directory or a
  conflicting target.
- **Unit:** proposal containing a feature and the PRD; assert the PRD content
  is exactly the proposed one.
- **Unit:** accept after the file was deleted externally → conflict.
- **Unit:** reject unknown id.

## Gotchas

- `Delete` needs a hash check too; deleting a file that changed after review is
  data loss.
- Apply order matters for a partial report; keep it deterministic and document
  it.
- Do not remove the proposal before a successful apply; otherwise a conflict
  leaves the person with nothing to inspect.
- Index refresh after acceptance can itself fail; report it in the result
  rather than losing the fact that files were written.
- No transaction exists. Say "files changed" in the result, not "all or
  nothing".
