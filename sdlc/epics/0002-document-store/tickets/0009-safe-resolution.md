---
status: done
depends_on: ["0008"]
---

# 0009: Symlink-safe path resolution

**Epic:** [0002 Document store](../description.md) · **Depends on:** 0008 · **Milestone:** M1

## Goal

Resolve a validated `DocPath` against the project root and prove it cannot
escape through `..`, a symlink, or a symlinked directory.

## Build

- `DocStore::new(root: PathBuf)` canonicalizes the root once at startup.
- `resolve(&self, path: &DocPath, mode) -> Result<PathBuf, StoreError>` where
  mode is `Existing` or `NewFile`.
  - `Existing`: canonicalize the joined path, ensure it starts with the root.
  - `NewFile`: canonicalize the parent directory, ensure it starts with the
    root, then join the file name.
- Reject any resolved path outside the canonical root with `NotAllowed`.
- Distinguish "parent directory missing" (`NotFound`) from "escapes root"
  (`NotAllowed`).
- Use `tokio::fs` or `spawn_blocking`; no blocking canonicalize in async.

## Already in place

- `DocPath` and the layout whitelist (0008).

## Acceptance criteria

- [x] A normal file inside the root resolves.
- [x] A symlinked file pointing outside the root is rejected.
- [x] A symlinked directory (`sdlc/features` → `/tmp/elsewhere`) is rejected.
- [x] A symlink that stays inside the root is allowed.
- [x] New files work when the parent exists and is inside the root.
- [x] The root itself may be reached through a symlink; the canonical root is
  the comparison base.

## Tests

- **Unit/integration:** build a temp project; create symlinks with
  `std::os::unix::fs::symlink` behind `#[cfg(unix)]`.
- **Security:** symlink file escape, symlink directory escape, symlink chain,
  `NewFile` under an escaping directory.
- **Unit:** missing parent returns `NotFound`, not a leaky I/O message.

## Gotchas

- Canonicalization requires the target to exist; handle `NewFile` separately or
  every create fails.
- Windows has junctions and different symlink permissions. Gate tests; do not
  claim Windows coverage the suite does not have.
- Record the accepted TOCTOU window in a code comment and in 0011's write path;
  do not pretend this is a security boundary against a local attacker who can
  already write the project.
- Do not return the resolved absolute path to clients; keep it internal and log
  it only at debug level.
