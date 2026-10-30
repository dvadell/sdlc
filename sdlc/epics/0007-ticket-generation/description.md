---
status: planned
---

# 0007: Ticket generation

*Tickets 0048–0053 · Milestone M5*

## Outcome

A feature spec can become one Markdown ticket file per unit of work, numbered
by the server, grouped under one epic, reviewed as a diff and saved only on
acceptance. Regeneration includes the feature's existing tickets in the chosen
epic so manual edits are visible in the diff. Tickets can also be created,
edited and deleted by hand.

## Why this order

Ticket metadata and epic grouping come first because generation depends on
knowing which tickets belong to which epic. The prompt and context come next,
then server-side numbering (the model never picks numbers), then regeneration,
then the hand-editing UI.

## Tickets

| # | Title | Depends on |
|---|-------|------------|
| [0048](tickets/0048-ticket-metadata.md) | Ticket metadata and epic grouping | 0012, 0013 |
| [0049](tickets/0049-generation-prompt.md) | Ticket generation prompt and context | 0035, 0044, 0048 |
| [0050](tickets/0050-ticket-numbering.md) | Server-side ticket numbering | 0014, 0041, 0049 |
| [0051](tickets/0051-regeneration.md) | Regenerate tickets against existing files | 0049, 0050 |
| [0052](tickets/0052-ticket-crud-ui.md) | Ticket create/edit/delete UI | 0020, 0025, 0048 |
| [0053](tickets/0053-ticket-list-ui.md) | Ticket list and status UI | 0022, 0030, 0048 |

## Exit criteria

- "Generate tickets from this feature" asks for an epic, produces a diff of new
  ticket files and writes nothing until accepted.
- New tickets get sequential numbers, a matching H1 and a path under the chosen
  epic's `tickets/` directory; existing tickets are not renumbered.
- Regeneration shows modifications and deletions against the feature's existing
  tickets in that epic.
- Tickets can be hand-created in an epic with an optional feature, then edited
  and deleted like other documents.
- Tickets whose parent epic has no `description.md` are visible as unassigned,
  not silently hidden.

## Risks

- The model may invent ticket paths or numbers; 0050 normalizes paths and
  numbering, and `DocPath` rejects anything outside
  `sdlc/epics/<epic>/tickets/`.
- Dependencies between newly created tickets cannot be expressed as final
  numbers until numbering happens. V1 records them in the body; see 0049.
- Regeneration can propose deleting manual work; the diff is the safeguard and
  deletion must be explicit.
- A feature may have tickets in more than one epic. Regeneration is scoped to
  the selected epic so it cannot silently rewrite tickets in another one.
