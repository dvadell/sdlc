---
status: done
depends_on: ["0010"]
---

# 0011: Atomic write with expected hash

**Epic:** [0002 Document store](../description.md) · **Depends on:** 0010 · **Milestone:** M1

## Goal

Save content only if the caller saw the current version, and never leave a
half-written file on disk.

## Build

- `Expectation` enum: `Existing(hash)` and `Missing`.
- `DocStore::write(&self, path, content, expectation) -> Result<String, StoreError>`
  returning the new hash.
  - `Existing`: read current hash; mismatch → `Conflict`; missing → `NotFound`.
  - `Missing`: fail with `AlreadyExists` if the file exists.
  - Write to a temp file in the same directory (`.<name>.tmp-<random>`), sync,
    then rename over the target; hash the exact bytes written.
- All writes go through path resolution and never follow a symlink to outside
  the root.
- Add a small helper `temp_path(dir)` and clean up the temp file on every error
  path.

## Already in place

- Read/hash and safe resolution (0009, 0010).

## Acceptance criteria

- [x] Correct hash writes the content and returns the new hash.
- [x] Stale hash returns `Conflict` and leaves the file untouched.
- [x] `Missing` creates; a second `Missing` returns `AlreadyExists`.
- [x] A failure while writing leaves no temp files and no partial target.
- [x] CRLF, trailing whitespace and empty content are written exactly.
- [x] File permissions are not widened beyond the platform default.

## Tests

- **Unit:** success, stale conflict, missing target, create collision.
- **Unit:** after a successful write the directory contains no `.tmp-*` file.
- **Unit:** simulate a failure (read-only directory, or a missing parent) and
  assert the original content is intact.
- **Unit:** overwrite keeps the file's permission bits instead of widening them.
- **Concurrency:** two writers with the same expected hash; exactly one
  succeeds and the other gets `Conflict`.

## Gotchas

- Rename over an existing file is atomic on Unix; on Windows it can fail. Use
  the platform-appropriate replace path and note it.
- Compute the conflict hash from what is on disk at check time; do not trust
  the expected hash in the error message.
- Record self-writes for watcher suppression only in 0055. This ticket must not
  grow a notification system.
- Do not hold a mutex across the `.await` for the file write.
- A crash between rename and index refresh is possible; 0029 owns the
  multi-file consequence.
