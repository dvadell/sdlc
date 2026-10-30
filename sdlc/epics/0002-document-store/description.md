---
status: planned
---

# 0002: Document store

*Tickets 0008–0017 · Milestone M1*

## Outcome

One component, `DocStore`, owns every file operation. It knows the fixed layout,
rejects unsafe paths including symlink escapes, reads and writes atomically with
content hashes, parses front matter, allocates numbers for epics, decisions and
tickets, lists the tree, and can scaffold a starter project. Nothing else
touches the filesystem.

## Why this order

Path validation and safe resolution come before any read or write. Hashing and
atomic writes come before routes. Numbering and templates come before feature,
epic and ticket creation. The store is fully unit-testable without HTTP, which
keeps the riskiest code out of the request layer.

## Tickets

| # | Title | Depends on |
|---|-------|------------|
| [0008](tickets/0008-doc-path.md) | DocPath and document layout whitelist | 0001 |
| [0009](tickets/0009-safe-resolution.md) | Symlink-safe path resolution | 0008 |
| [0010](tickets/0010-read-and-hash.md) | Read a document with content hash | 0009 |
| [0011](tickets/0011-atomic-write.md) | Atomic write with expected hash | 0010 |
| [0012](tickets/0012-frontmatter.md) | YAML front matter parsing | 0001 |
| [0013](tickets/0013-tree.md) | Document tree listing | 0010, 0012 |
| [0014](tickets/0014-numbering.md) | Sequential number allocation | 0008 |
| [0015](tickets/0015-create.md) | Create documents from templates | 0011, 0012, 0014 |
| [0016](tickets/0016-rename-delete.md) | Rename and delete documents | 0015 |
| [0017](tickets/0017-scaffold.md) | Scaffold the starter layout | 0011, 0015 |

## Exit criteria

- Every external path string must become a `DocPath`; tests prove traversal,
  backslashes, absolute paths and symlink escapes fail.
- A stale hash never overwrites a file.
- A scaffolded project has all fixed documents, valid front matter and the PRD
  index markers.
- Numbered epics, decisions and tickets never collide, including concurrent
  creates.
- `just check` passes and the security negative tests exist.

## Risks

- **TOCTOU on symlinks:** canonicalize-then-open leaves a small window. Accept
  it for a local single-user tool and document it; do not build a half-baked
  sandbox.
- **Front matter crate churn:** `serde_yaml` is archived. Ticket 0012 evaluates
  current options and records the choice before code depends on it.
- **Empty directories:** git does not track them. Scaffolding creates the
  top-level directories, but an epic's `tickets/` is created with the epic; the
  store must not assume either exists.
