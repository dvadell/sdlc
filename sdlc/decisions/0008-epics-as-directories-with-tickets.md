---
status: accepted
---

# 0008: Epics as directories with tickets

## Context

The document set needs epics and tickets to group without a second field that can
disagree with the path (see architecture sections 2 and 6).

## Decision

Model epics as directories with a `description.md` and a `tickets/`
subdirectory; a ticket's path determines its exactly one epic.

## Consequences

- Epic membership is the path, never a stored field.
- Numbering allocates ticket numbers inside their epic directory.
