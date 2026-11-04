---
status: done
depends_on: ["0017", "0021"]
---

# 0026: Empty-layout onboarding

**Epic:** [0003 Document API and editor](../description.md) · **Depends on:** 0017, 0021 · **Milestone:** M1

## Goal

When the project has no layout, explain the situation and offer to create the
starter documents, without doing it behind the person's back.

## Build

- Use `layout_exists` from `/api/tree`; if false, replace the sidebar/editor
  area with an empty state: one sentence, the path that would be created, and a
  "Create starter documents" button.
- On click, `POST /api/scaffold`, then refresh config/tree and open `sdlc/PRD.md`.
- If the layout is partial but all core files are missing, show the same
  affordance; the store already skips existing files.
- Show scaffold errors in the banner and allow retry.
- Do not create anything on page load.

## Already in place

- Scaffold endpoint and store (0017, 0020); shell.

## Acceptance criteria

- [x] Empty project shows the onboarding state, not an empty sidebar.
- [x] Nothing is written until the button is clicked.
- [x] A successful scaffold opens the PRD and lists all core files.
- [x] A second click/reload no longer shows onboarding.
- [x] Existing custom files survive a partial scaffold.
- [x] A read-only project shows a clear error.

## Tests

- **Integration:** tree `layout_exists` false on empty, true after scaffold.
- **Manual:** open a fresh temp directory; confirm the empty state; click
  create; inspect the files.
- **Manual:** pre-create only `sdlc/PRD.md`, scaffold, confirm it is unchanged.
- **Manual:** make the directory read-only and confirm the error banner.

## Gotchas

- `layout_exists` must not mean "all core files exist"; a partial project still
  needs the cleanup path.
- Do not hard-code `sdlc/` in the UI text; read what the server reports if it
  ever becomes configurable.
- After scaffolding, the tree must be refetched, not patched locally.
- Scope this ticket to the empty state; richer document creation stays in 0025.
