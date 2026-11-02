---
status: done
depends_on: ["0007", "0011"]
---

# 0019: PUT save route

**Epic:** [0003 Document API and editor](../description.md) · **Depends on:** 0007, 0011 · **Milestone:** M1

## Goal

Save a document with optimistic concurrency, returning the new hash and a
conflict instead of clobbering an external edit.

## Build

- `PUT /api/docs/{*path}` with body `{ content: string, expected_hash: string }`
  → `{ hash: string }`.
- Parse the path first, reject blank `expected_hash`, then call
  `store.write(..., Expectation::Existing(hash))`.
- Map `Conflict` to 409 `conflict`; `NotFound` to 404.
- Set an explicit body limit (2 MiB is enough for these documents) and let axum
  return 413 for larger bodies.
- Guard already requires `application/json` on PUT (0006); do not repeat it.

## Already in place

- Atomic write, error mapping, guard, `AppState`.

## Acceptance criteria

- [x] Correct hash saves and returns the new hash.
- [x] The response hash is computed from the bytes written.
- [x] Stale hash → 409 and the file is unchanged.
- [x] Missing document → 404; invalid path → 400.
- [x] Empty `expected_hash` → 400 before touching disk.
- [x] Oversized body → 413 with the standard error shape.
- [x] Content type other than JSON → 415 from the guard.

## Tests

- **Integration:** save, read back, hash matches; save again with the old hash
  → 409.
- **Integration:** external write between read and save → 409.
- **Integration:** missing document, invalid path, empty hash.
- **Integration:** 3 MiB body → 413.
- **Unit:** error body for conflict contains no absolute path.

## Gotchas

- Do not return the current disk hash on conflict; the UI reloads with `GET`.
  Keeping the error shape uniform avoids a second contract.
- Save must not refresh the PRD index yet; 0029 owns that.
- The hash after save must be what future saves send back; the UI updating from
  anywhere else creates false conflicts.
- Check that axum's default body limit does not return HTML; configure it to
  use the JSON error shape.
