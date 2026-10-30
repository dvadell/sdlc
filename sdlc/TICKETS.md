# sdlc implementation plan

This file breaks the work described in [`PRD.md`](PRD.md) into epics and small
tickets. Read [`architecture.md`](architecture.md) and
[`conventions.md`](conventions.md) before starting a ticket.

This repository keeps its documents under `sdlc/`. Each epic is a directory
under `sdlc/epics/` with a `description.md` and a `tickets/` subdirectory;
decisions live in `sdlc/decisions/`. This file is a human index; sdlc ignores
it.

## Epics

| Epic | Tickets | Milestone | Outcome |
|------|---------|-----------|---------|
| [0001 Foundation](epics/0001-foundation/description.md) | 0001–0007 | M1 | A guarded loopback server with embedded assets and configuration |
| [0002 Document store](epics/0002-document-store/description.md) | 0008–0017 | M1 | Safe, hash-checked file access and scaffolding |
| [0003 Document API and editor](epics/0003-document-api-and-editor/description.md) | 0018–0027 | M1 | Browse, edit and save documents in the browser |
| [0004 Features, epics and decisions](epics/0004-features-epics-and-decisions/description.md) | 0028–0032 | M2 | Feature CRUD with a live PRD index, epic status, and ADR workflow |
| [0005 AI chat](epics/0005-ai-chat/description.md) | 0033–0039 | M3 | Streaming chat with document context |
| [0006 Proposals and prompts](epics/0006-proposals-and-prompts/description.md) | 0040–0047 | M4 | Reviewable AI edits and built-in prompts |
| [0007 Ticket generation](epics/0007-ticket-generation/description.md) | 0048–0053 | M5 | Feature specs become numbered ticket files grouped by epic |
| [0008 Polish and release](epics/0008-polish-and-release/description.md) | 0054–0061 | M6 | External changes, warnings, docs and release builds |

## How to use a ticket

- One ticket should fit one sitting. If it grows, split it and renumber nothing.
- New tickets take the next free global number; new epics take the next free
  epic number. The two sequences are independent.
- Ticket numbers are stable and global; never reuse or renumber one.
- A ticket lives at `sdlc/epics/<epic>/tickets/<ticket>.md`. Its path determines
  its epic, so there is no `epic:` front matter key.
- A ticket may name the feature it came from with `feature: <slug>`, but the
  feature is optional.
- `depends_on` lists ticket ids, quoted to keep the leading zeros:
  `depends_on: ["0008", "0012"]`. A ticket is ready when every dependency is
  `done`.
- Update `status` to `in-progress` when you start and `done` only when the
  acceptance criteria and tests are complete.
- Epic numbers are stable too. Renaming an epic keeps its number and moves its
  tickets with it. Deleting an epic is refused while it still has tickets.

## Definition of done (every ticket)

- Acceptance criteria are met and the listed tests exist and pass.
- `just check` passes (fmt, clippy with `-D warnings`, tests).
- No `unwrap()`/`expect()` in non-test code, no `panic!` in request paths, and
  no filesystem path is built from request data outside `DocPath`.
- New behaviour is reflected in the docs it touches (`architecture.md`,
  `conventions.md`, `README.md`). Change the convention first if the code and
  the convention disagree.
- The person owns git: do not commit, push or branch.

## Test layers

- **Unit tests** live beside the code and cover parsing, validation, numbering,
  epic grouping, diffs and index generation.
- **Integration tests** live in `tests/`, use a `tempfile::TempDir` project and a
  `wiremock` fake AI server, and never call a real AI service.
- **Security tests** are negative tests in the same layers: traversal, encoded
  traversal, symlink escape, bad Host/Origin, wrong content type.
- **Frontend** has no automated suite in v1. Tickets list manual checks, with a
  browser-only self-test page where a pure function is worth exercising.

## Known deliberate limits

These are v1 non-goals and should not become tickets: no auth, no network
access, no chat persistence, no context-window management, no per-hunk diffs,
no database, no source-code access for the AI.
