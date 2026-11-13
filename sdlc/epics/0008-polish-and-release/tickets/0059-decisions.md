---
status: done
depends_on: ["0001"]
---

# 0059: Record the architecture decisions

**Epic:** [0008 Polish and release](../description.md) · **Depends on:** 0001 · **Milestone:** M6

## Goal

Turn architecture section 12 and the front matter crate choice from 0012 into
real ADR files, so the reasoning survives.

## Build

- Create `sdlc/decisions/NNNN-title.md` for this repository's docs root:
  1. Use `axum` on `tokio`.
  2. Files as the only storage; no database.
  3. Proposals via text blocks, not tool calling.
  4. Server computes diffs from full-file content.
  5. Stateless chat endpoint; history lives in the browser.
  6. Plain HTML/CSS/JavaScript with no build step, two vendored libraries.
  7. Optimistic concurrency with content hashes instead of file locks.
  8. Epics as directories with `description.md` and `tickets/`; a ticket's path
     determines its exactly one epic.
  9. The YAML front matter crate chosen in 0012 (record the alternatives
     considered). If 0012 already wrote ADR `0009`, review it instead of
     creating a duplicate.
- Use the decision template from `conventions.md`: front matter `status:
  accepted`, one H1 `# NNNN: Title`, Context/Decision/Consequences.
- Keep each file short; link to architecture sections instead of copying them.
- Add a one-line index or link from `sdlc/architecture.md` section 12 if it
  still says "should get a file".

## Already in place

- Architecture section 12 and the docs root.

## Acceptance criteria

- [ ] Nine decision files with unique sequential numbers.
- [ ] Every file parses with the front matter parser and has one H1.
- [ ] Each names the context, the decision and at least one consequence.
- [ ] The YAML crate decision records what was rejected and why.
- [ ] `sdlc/architecture.md` no longer implies the ADRs are missing.

## Tests

- **Manual/scripted:** list the directory; numbers are contiguous and names are
  kebab-case.
- **Unit (optional):** run `DocPath::parse` on each path in a temp layout with
  `sdlc/` substituted.
- **Manual:** render one file on GitHub; front matter is hidden and the H1 is
  correct.

## Gotchas

- This repository keeps docs in `sdlc/`; do not create a parallel `docs/` or a
  second decisions directory.
- Decision numbers here are documentation, not the product's allocator output;
  keep them stable once written.
- Do not rewrite history in `architecture.md`; adding links is enough.
- The front matter crate may change; update ADR 0009 rather than adding a
  second decision for the same choice.
