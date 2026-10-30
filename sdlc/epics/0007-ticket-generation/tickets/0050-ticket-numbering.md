---
status: todo
depends_on: ["0014", "0041", "0049"]
---

# 0050: Server-side ticket numbering

**Epic:** [0007 Ticket generation](../description.md) · **Depends on:** 0014, 0041, 0049 · **Milestone:** M5

## Goal

Assign sequential numbers to proposed new tickets, rewrite their paths and H1s,
and leave existing tickets untouched.

## Build

- In proposal creation, **before** 0041 parses paths and rejects duplicates,
  when `ticket_context` is present:
  - For each `Create` whose raw path is under the selected epic's `tickets/`
    directory (or uses the prompt's `sdlc/epics/<epic>/tickets/` placeholder):
    - If the file name has no `NNNN-` prefix (or has `0000-`), allocate the
      next global number and replace the path with
      `sdlc/epics/<ticket_context.epic>/tickets/NNNN-slug.md`.
    - The model's slug is slugified again; reject if empty.
    - If the H1 is `# Title`, rewrite it to `# NNNN: Title`. If it already has
      a different number, use the allocated one; never trust the model.
    - Remove any `epic:` front matter key the model added; the path, not the
      file, selects the epic. Emit a warning when a key was removed.
    - If `ticket_context.feature` is set, ensure `feature: <slug>` is present
      and agrees; add it when absent.
  - Allocate for all creates in block order, so numbers are sequential within
    the proposal. Do not reserve numbers across proposals.
- Existing modifications/deletes are not renumbered; a modify path must already
  be valid.
- A proposal that would exceed `9999` fails cleanly.
- Without `ticket_context`, keep today's behaviour: explicit numbered paths are
  accepted as-is, unnumbered paths are rejected by `DocPath`.

## Already in place

- Number allocation, proposal building, ticket context from 0049.

## Acceptance criteria

- [ ] Two unnumbered creates become `NNNN` and `NNNN+1` in the selected epic's
      `tickets/` directory.
- [ ] H1s gain the same numbers.
- [ ] Removed `epic` keys do not reappear; optional `feature` matches the
      selected feature.
- [ ] A model-supplied `0007-` prefix is replaced with the next free number.
- [ ] Existing ticket paths are not renumbered.
- [ ] Numbering is stable and collision-free for one proposal.
- [ ] An invalid or non-ticket path still rejects the whole proposal.

## Tests

- **Unit:** one, two and three creates; numbering follows block order.
- **Unit:** H1 absent, H1 numbered, H1 with extra `#`s.
- **Unit:** model slug with spaces/uppercase; empty slug rejected.
- **Unit:** front matter missing, model-added epic key removed, absent/wrong
  feature, unknown keys preserved.
- **Integration:** accept the proposal; files exist under the selected epic with
  matching names, H1s and no `epic` key.
- **Unit:** boundary at 9999.

## Gotchas

- Numbering is reserved only in memory until accept; a second proposal can pick
  the same number. Its create then conflicts at accept time; that is the
  designed safety net.
- Rewriting front matter must preserve unknown keys and the rest of the
  document.
- Paths must include the epic directory. If the model writes a flat
  `sdlc/tickets/...` path, either rewrite it into `ticket_context.epic` or reject
  it; never create a flat ticket.
- Do not renumber when the person is editing tickets by hand (0052); only
  generated `Create` blocks in a ticket context.
- The diff must show the final numbered path, or the review is misleading.
