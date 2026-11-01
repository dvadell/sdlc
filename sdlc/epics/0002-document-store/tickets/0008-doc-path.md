---
status: done
depends_on: ["0001"]
---

# 0008: DocPath and document layout whitelist

**Epic:** [0002 Document store](../description.md) · **Depends on:** 0001 · **Milestone:** M1

## Goal

Make invalid document paths unrepresentable. Every path from a request or the
AI becomes a `DocPath` before anything touches disk.

## Build

- `src/store/path.rs`: `DocPath(PathBuf)` with `parse(&str)`, `as_path()`.
- `src/store/layout.rs`: `is_valid(path)` and a `DocKind` enum.
- Allowed paths, exactly:
  - `sdlc/PRD.md`, `sdlc/architecture.md`, `sdlc/conventions.md`, `AGENTS.md`
  - `sdlc/epics/<epic>/description.md`
  - `sdlc/epics/<epic>/tickets/<ticket>.md`
  - `sdlc/decisions/NNNN-slug.md`
  - `sdlc/features/slug.md` (one segment, no subdirectories)
- Parse rules: non-empty, relative, only `Component::Normal`, no `\`, no empty
  segments, `.md` extension, lowercase kebab-case directory and file names.
  Epic directories and ticket files require a four-digit prefix. There is
  exactly one nesting level: tickets live only in an epic's `tickets/`
  directory. `AGENTS.md` is the only root document.
- `DocKind` reports `Core(CoreDoc)`, `Epic`, `Feature`, `Decision`, `Ticket`.
- Add `src/store/mod.rs` with a `StoreError` enum (`NotAllowed`, `NotFound`,
  `AlreadyExists`, `Conflict`, `InvalidUtf8`, `Io`) using `thiserror`. HTTP
  mapping is added in 0018; do not couple the store to axum.

## Already in place

- Crate skeleton only; no store module yet.

## Acceptance criteria

- [x] Every listed path parses; each kind is identified correctly.
- [x] `..`, `.`, absolute paths, `sdlc/../AGENTS.md`, `./sdlc/PRD.md`,
  backslashes, empty strings, trailing slashes and nested feature paths fail.
- [x] Unknown directories, non-Markdown files and uppercase extensions fail.
- [x] An unnumbered epic directory, decision or ticket fails
      (`sdlc/epics/use-axum/description.md`, `sdlc/decisions/use-axum.md`).
- [x] `sdlc/epics/0001-foundation/tickets/0001-project.md` is a ticket; the same
  file at `sdlc/epics/0001-foundation/0001-project.md` is rejected.
- [x] A ticket outside an epic's `tickets/` directory is rejected.
- [x] Creation of a new path is possible without the file existing.

## Tests

- **Unit:** table of valid paths with expected `DocKind`, including an epic
  description and a nested ticket.
- **Unit:** table of invalid paths with the expected `NotAllowed` error.
- **Security:** `%2e%2e%2f`, `..%2f`, `sdlc/features/../../AGENTS.md`,
  `/etc/passwd`, `C:\sdlc\PRD.md`.
- **Unit:** four-digit boundaries `0000` and `9999`; five digits fail.

## Gotchas

- Do not canonicalize here: new files do not exist yet. Existence and symlink
  checks belong to 0009.
- Axum may pass an encoded path still containing `%2F`; reject anything that
  does not split into the expected components rather than trying to decode it
  here.
- Keep the rules in one place. `DocStore` must never construct a path from
  request data on its own.
- Existing files that violate the naming rules will simply not be listed; that
  is acceptable in v1 and should be logged at debug level in 0013.
