---
status: todo
depends_on: ["0007", "0042"]
---

# 0043: Proposal API routes

**Epic:** [0006 Proposals and prompts](../description.md) · **Depends on:** 0007, 0042 · **Milestone:** M4

## Goal

Expose accept and reject over HTTP with the standard error shape.

## Build

- `POST /api/proposals/{id}/accept` → `{ applied: [path], warnings?: [] }` or
  `409 conflict`, `404 not_found`, or `500` with a partial report.
- `POST /api/proposals/{id}/reject` → `{ rejected: true }`.
- Both accept an empty JSON object; the guard requires `application/json`.
- Reject/accept id is opaque; do not parse it, just look it up.
- Map `Partial` to a response that includes applied paths and the failed path,
  with a stable code such as `partial_apply`.
- Wire the router under `/api`; no new state beyond `Arc<ProposalService>`.

## Already in place

- Proposal service, `AppState`, guard, error mapping.

## Acceptance criteria

- [ ] Accept applies and returns applied paths.
- [ ] Reject removes and returns success.
- [ ] Unknown id → 404.
- [ ] Conflict → 409, no files changed.
- [ ] Partial apply → non-2xx with `applied` and `failed` fields.
- [ ] Missing JSON content type → 415 from the guard.

## Tests

- **Integration:** seed a proposal through the service in the test harness,
  then call both routes.
- **Integration:** accept twice; second is 404.
- **Integration:** simulate a stale file before accept; 409 and no change.
- **Integration:** unknown id, missing content type.
- **Integration:** response body shape for partial apply.

## Gotchas

- Do not create a test-only HTTP route to seed proposals; use the public
  service from an integration test helper.
- The error shape is `{error:{code,message}}`; partial details may sit beside
  it or inside a dedicated field. Pick one and keep it stable for 0045.
- HTTP 500 for a partial apply is misleading if some files changed; use 409 or
  500 with `partial_apply` and document it in the UI ticket.
- Accept can take a moment for many files; do not impose a tiny request
  timeout.
