---
status: accepted
---

# 0007: Optimistic concurrency with content hashes

## Context

The editor already saves with the content hash the client last saw, so a file lock
is not required for a local single-user tool (see architecture sections 3.2 and 7).

## Decision

Use optimistic concurrency with content hashes instead of file locks.

## Consequences

- A stale save fails with a conflict instead of overwriting.
- Proposals re-check hashes before applying and write all changes together or
  none.
