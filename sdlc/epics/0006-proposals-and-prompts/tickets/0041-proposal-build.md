---
status: done
depends_on: ["0010", "0040"]
---

# 0041: Build proposals and diffs

**Epic:** [0006 Proposals and prompts](../description.md) · **Depends on:** 0010, 0040 · **Milestone:** M4

## Goal

Turn parsed blocks into a validated, reviewable proposal: classify each change,
remember the hash it was based on, and compute a unified diff.

## Build

- `src/proposal.rs`:
  ```rust
  pub struct Change { path: DocPath, action: Action, new_content: Option<String>,
                      base_hash: Option<String>, diff: String }
  pub enum Action { Create, Modify, Delete }
  pub struct Proposal { id: String, changes: Vec<Change> }
  ```
- `ProposalService::create(blocks) -> Result<Proposal, ProposalError>`:
  1. Parse every block path into `DocPath`; any failure rejects the whole
     proposal. Ticket-context proposals are normalized by 0050 before this
     step, so keep the raw block list available rather than consuming it in
     the parser.
  2. Reject duplicate paths.
  3. `Write`: read current content. Exists → `Modify` with `base_hash`; missing
     → `Create` with `None`. `Delete`: must exist, `base_hash` set.
  4. Compute a unified diff with `similar` (3 lines of context), headers
     `--- a/<path>` / `+++ b/<path>`. Create diffs from empty; delete diffs to
     empty.
  5. Store in a `Mutex<HashMap<String, Proposal>>`; ids are UUIDs.
- Proposals live only in memory and expire on restart.
- Reject an empty proposal (no changes).

## Already in place

- Block parser, store reads and hashes, `DocPath`.

## Acceptance criteria

- [ ] Write to a missing file is `Create`; to an existing file `Modify`.
- [ ] Delete requires an existing file.
- [ ] One invalid path rejects the entire proposal.
- [ ] Duplicate paths are rejected.
- [ ] Diff shows additions, deletions and context; headers use relative paths.
- [ ] Same content → empty diff but a valid proposal (or reject as no-op; pick
      one and test).
- [ ] IDs are unique.

## Tests

- **Unit:** create/modify/delete classification.
- **Unit:** invalid path list, duplicates, delete missing, empty block list.
- **Unit:** diff content for a small before/after; check `+`/`-` counts.
- **Unit:** path with backslashes, traversal, absolute path.
- **Unit:** large file diff runs off the async worker (or is bounded).

## Gotchas

- Read current content once per path; do not read again during diff and accept.
- Store `base_hash` for creates as `None`; accept must re-check that the file
  is still missing.
- Diffs include a trailing-newline marker handling; keep it consistent with the
  UI's line classifier.
- Do not log file contents; diffs contain document text.
- The proposal is based on disk at creation time, not on the open editor's
  buffer. The UI must say so.
