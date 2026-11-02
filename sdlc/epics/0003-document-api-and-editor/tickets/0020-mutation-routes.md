---
status: done
depends_on: ["0007", "0015", "0016", "0017"]
---

# 0020: Create, move, delete and scaffold routes

**Epic:** [0003 Document API and editor](../description.md) · **Depends on:** 0007, 0015, 0016, 0017 · **Milestone:** M1

## Goal

Expose the store's lifecycle operations over HTTP.

## Build

- `POST /api/docs` with `{ kind, title, epic?, feature? }` → 201
  `{ path, hash }`. `kind` is `epic | feature | decision | ticket`. A ticket
  requires `epic` (the target directory); `feature` is optional.
- `POST /api/docs/move` with `{ from, to }` → `{ path }`.
- `DELETE /api/docs/{*path}` → 200 `{ orphaned_tickets?: number }`. A non-empty
  epic returns 409 and is not deleted.
- `POST /api/scaffold` (empty JSON object) → `{ created, skipped }`.
- Parse all request paths into `DocPath`; never pass raw strings to the store.
- Status mapping: invalid input 400, duplicate/collision 409, missing 404,
  core deletion 400 with code `not_allowed`.
- Ticket responses derive `epic` from the path, not from request or front
  matter.

## Already in place

- Store create/move/delete/scaffold and the read/save routes.

## Acceptance criteria

- [x] Each kind can be created and appears in the tree.
- [x] Epic creation defaults to `planned`, gets the next epic number, and its
  `description.md` lands in a new directory.
- [x] Ticket creation without an epic → 400; unknown epic → 400/404; a
  successful ticket path is under that epic's `tickets/`.
- [x] Ticket with an unknown feature → 400; omitting the feature is allowed.
- [x] Duplicate feature creation → 409, original untouched.
- [x] Move validates both paths and keeps numbered prefixes; moving an epic
  moves its tickets with it.
- [x] Deleting a core document → 400 `not_allowed`.
- [x] Deleting a non-empty epic → 409 and nothing is removed.
- [x] Deleting a feature reports how many tickets reference it.
- [x] Scaffold is idempotent and returns created/skipped.

## Tests

- **Integration:** one test per endpoint, happy and unhappy path.
- **Security:** create title containing `/`, `..`, newline; move across kinds;
  delete `AGENTS.md`.
- **Integration:** create a ticket with and without a feature; missing/unknown
  epic.
- **Integration:** scaffold a temp project, then tree; second call created is
  empty.
- **Integration:** check response status codes, not just bodies.

## Gotchas

- `POST /api/docs` and `GET /api/docs/{*path}` share a prefix; confirm axum
  routes the exact path first.
- A move response must carry the new path, or the UI cannot update selection.
- `POST /api/scaffold` with no body still needs the JSON content type from the
  UI; send `{}` explicitly.
- Do not add a bulk-delete or recursive delete. Epics own directories, and a
  non-empty epic is refused rather than silently deleting its tickets.
- Ticket paths are nested one level deeper than other documents; do not use a
  flat `sdlc/tickets/` assumption in route tests.
