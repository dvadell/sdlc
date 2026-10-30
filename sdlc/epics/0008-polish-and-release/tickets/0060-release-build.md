---
status: todo
depends_on: ["0005", "0020"]
---

# 0060: Release build and embedded asset check

**Epic:** [0008 Polish and release](../description.md) · **Depends on:** 0005, 0020 · **Milestone:** M6

## Goal

Prove the release binary is self-contained: it serves the frontend from an
arbitrary working directory with no `frontend/` folder beside it.

## Build

- Add/verify `just release` = `cargo build --release`.
- Add a release profile with `lto = "thin"`, `strip = true` (platform allowing)
  and note the trade-off; measure the binary and startup time.
- Ensure `rust-embed` embeds assets in release and does not read disk. Debug
  behaviour must stay disk-based.
- Add `--version` output (package version) if not already present.
- Write `scripts/check-release.sh` (or a `just` recipe) that:
  1. builds release;
  2. copies the binary to a fresh temp dir with no repository;
  3. starts it with `--dir <empty temp project>` on port `0`;
  4. requests `/` and `/api/config`;
  5. stops it and fails on non-200 or missing markers.
- Document per-platform builds (`sdlc`, `sdlc.exe`) and that there is no Node
  step.

## Already in place

- Embedded asset route, all routes, README.

## Acceptance criteria

- [ ] The release binary runs outside the repository and serves the UI.
- [ ] No frontend file is read from disk at runtime in release.
- [ ] `--version` prints the package version.
- [ ] `just release` and the check script pass from a clean checkout.
- [ ] Startup is effectively instant and idle memory is small enough to state
      a measured number in the README.
- [ ] Windows/macOS differences are documented, even if only Linux is tested
      here.

## Tests

- **Manual/scripted:** the check script above; assert the page contains a known
  string from `index.html` and the config JSON parses.
- **Manual:** move/rename the repository's `frontend/` directory and run the
  release binary from `/tmp`; still works.
- **Manual:** measure `time ./sdlc --help` and startup logs; note in README.
- **Manual:** build in debug and confirm frontend edits still need only a
  refresh.

## Gotchas

- Debug and release asset behaviour differ by design; a passing debug test
  proves nothing about embedding.
- A relative `--dir` path is resolved against the working directory; the script
  must use an absolute temp path.
- Stripping symbols can hamper panic backtraces; acceptable for release, but
  keep an unstripped CI artifact if debugging matters.
- Do not ship `target/` or editor files; check the tarball contents.
