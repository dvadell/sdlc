---
status: todo
depends_on: ["0056", "0057", "0058", "0059", "0060"]
---

# 0061: Manual release checklist and QA pass

**Epic:** [0008 Polish and release](../description.md) · **Depends on:** 0056–0060 · **Milestone:** M6

## Goal

Run the whole product against a fresh project, with and without an AI backend,
and record the result. This is the last gate before calling v1 done.

## Build

- Write `MANUAL_TESTING.md` at the repository root (not inside the managed
  layout) with checkbox sections:
  - **Fresh project:** empty dir → onboarding → scaffold → all core docs open.
  - **Editing:** create/rename/delete epic, feature, decision and ticket; core
    protected; non-empty epic delete refusal; unsaved indicator; Ctrl/Cmd+S;
    CRLF preservation.
  - **Conflicts:** external edit then save; reload vs overwrite; deleted file.
  - **Index:** feature status/title/rename/delete updates the PRD region only.
  - **AI unconfigured:** editing works; setup instructions name the env vars.
  - **AI configured (local):** stream, stop, error, no history after reload.
  - **Proposals:** accept, reject, conflicting accept, partial failure,
    malformed block warning, invalid path rejection.
  - **AGENTS.md:** trim prompt, diff, accept; `CLAUDE.md` proposal rejected.
  - **Tickets:** generate into a chosen epic, review, accept, hand-edit,
    regenerate, malformed-epic unassigned group.
  - **Watcher:** external edit reloads a clean doc, warns on a dirty one, app
      saves are silent.
  - **Warnings:** over-limit PRD, code fence, stack heading.
  - **Security:** XSS in a document and in a model reply; bad Host/Origin via
      curl; traversal attempts.
  - **Release:** run the 0060 script and the binary outside the repo.
- Include the date, commit, platform, browser and AI server model in a header
  filled per run.
- Run it; file each failure as a new ticket instead of fixing it silently in
  this one. Re-run the affected section after fixes.

## Already in place

- Every feature ticket in the plan.

## Acceptance criteria

- [ ] `MANUAL_TESTING.md` exists and covers every section above.
- [ ] A full run is completed on a clean temp project.
- [ ] Both AI-configured and unconfigured runs are recorded.
- [ ] All failures have a ticket or a documented accepted limitation.
- [ ] `just check` passes on the tested commit.
- [ ] The success criteria in the PRD are each checked off with evidence.

## Tests

- **Manual:** the checklist itself.
- **Manual:** repeat the fresh-project path twice to confirm scaffolding is
  idempotent.
- **Manual:** use a second browser profile for the single-tab limitation note.
- **Manual:** verify no secrets appear in the README, logs or config response.

## Gotchas

- Testing only against a remote commercial model hides local-server quirks;
  use an OpenAI-compatible local server for at least the stream/proposal path.
- A green unit suite does not prove the UI; do not skip the browser sections.
- Do not mark this done with known broken core flows; split and schedule the
  fixes.
- Keep the checklist honest about what was not tested (Windows, Safari, huge
  documents).
