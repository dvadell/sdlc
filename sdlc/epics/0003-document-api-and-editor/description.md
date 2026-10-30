---
status: planned
---

# 0003: Document API and editor

*Tickets 0018–0027 · Milestone M1*

## Outcome

A usable editor: the sidebar lists the project, the main area loads and saves a
document with conflict detection, preview renders sanitized Markdown, and the
person can create, rename and delete epics, features, decisions and tickets. The
core files are editable but not deletable.

## Why this order

Routes first, then the shell, then one vertical slice (tree → select → edit →
save), then the mutation actions and the failure states. Each ticket is
testable in the browser as soon as it lands.

## Tickets

| # | Title | Depends on |
|---|-------|------------|
| [0018](tickets/0018-read-routes.md) | GET tree and document routes | 0007, 0010, 0013 |
| [0019](tickets/0019-save-route.md) | PUT save route | 0007, 0011 |
| [0020](tickets/0020-mutation-routes.md) | Create, move, delete and scaffold routes | 0007, 0015, 0016, 0017 |
| [0021](tickets/0021-frontend-shell.md) | Frontend shell and API module | 0005 |
| [0022](tickets/0022-sidebar.md) | Sidebar document tree | 0018, 0021 |
| [0023](tickets/0023-editor.md) | Editor load, save and dirty state | 0019, 0021 |
| [0024](tickets/0024-preview.md) | Preview toggle with vendored libraries | 0021 |
| [0025](tickets/0025-crud-ui.md) | Create, rename and delete UI | 0020, 0022, 0023 |
| [0026](tickets/0026-onboarding.md) | Empty-layout onboarding | 0017, 0021 |
| [0027](tickets/0027-conflict-ui.md) | Save conflict UI | 0023 |

## Exit criteria

- Open a scaffolded project, select each document, edit, save, reload.
- A stale save returns a conflict and the UI offers reload or explicit
  overwrite.
- Preview sanitizes injected HTML and scripts.
- Create/rename/delete work for epics, features, decisions and tickets; core
  files cannot be deleted.
- All API tests from 0018–0020 exist; frontend checks are manual as v1 allows.

## Risks

- Browser line-ending normalization can rewrite CRLF files; 0023 handles it.
- A catch-all asset route can hide API 404s; already tested in 0005.
- The state object can drift from disk; only update the stored hash from a
  server response.
