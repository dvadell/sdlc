---
status: accepted
---

# 0002: Files as the only storage

## Context

The product manages one project's Markdown documents; a database would put a
second source of truth next to the files on disk.

## Decision

Keep the files as the only storage; no database.

## Consequences

- No schema or migration can drift away from the files.
- Cheap in-memory state (open proposals, file hashes) is safe to lose; conflicts
  surface as hash mismatches on save.
- Documents stay plain Git- and editor-friendly Markdown.
