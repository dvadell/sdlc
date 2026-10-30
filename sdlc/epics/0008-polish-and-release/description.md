---
status: planned
---

# 0008: Polish and release

*Tickets 0054–0061 · Milestone M6*

## Outcome

The app notices external file changes, warns about a bloated PRD, documents its
security and privacy behaviour, records the architecture decisions, and ships
as a single verified release binary. A manual checklist ties the whole product
together.

## Why this order

The watcher needs suppression before it is useful, and the client needs the
watcher. Warnings and documentation are independent and can run in parallel
once the editor exists. Decisions are documentation-only. Release verification
depends on assets and routes; the checklist is last because it exercises
everything.

## Tickets

| # | Title | Depends on |
|---|-------|------------|
| [0054](tickets/0054-file-watcher.md) | File watcher and event hub | 0002 |
| [0055](tickets/0055-self-write-suppression.md) | Suppress self-write events | 0011, 0054 |
| [0056](tickets/0056-events-client.md) | Events SSE route and client reload | 0027, 0055 |
| [0057](tickets/0057-prd-warnings.md) | PRD size and smell warnings | 0002, 0010, 0023 |
| [0058](tickets/0058-readme-privacy.md) | README and privacy notice | 0021, 0038 |
| [0059](tickets/0059-decisions.md) | Record the architecture decisions | 0001 |
| [0060](tickets/0060-release-build.md) | Release build and embedded asset check | 0005, 0020 |
| [0061](tickets/0061-manual-checklist.md) | Manual release checklist and QA pass | 0056, 0057, 0058, 0059, 0060 |

## Exit criteria

- An external edit to a clean open document reloads it; a dirty document asks.
- sdlc's own writes do not produce notifications.
- A PRD over the configured size shows a non-blocking warning, as do code
  fences and stack-related headings.
- README and UI state clearly that document content goes to the configured AI
  host.
- Nine architecture decisions, including the epic model and the front matter
  crate choice, exist as ADRs.
- A release binary serves the UI from an empty working directory.
- The manual checklist is complete for the release candidate.

## Risks

- Filesystem notification timing is platform- and editor-dependent; tests must
  use generous timeouts and tolerate atomic-rename patterns.
- Self-write suppression is global, so a second browser tab will not be told
  about a save from the first. Document this single-user limitation.
- Release embedding is the classic "works in debug, blank page in release"
  failure; test it explicitly.
