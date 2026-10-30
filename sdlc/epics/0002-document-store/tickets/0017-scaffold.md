---
status: todo
depends_on: ["0011", "0015"]
---

# 0017: Scaffold the starter layout

**Epic:** [0002 Document store](../description.md) · **Depends on:** 0011, 0015 · **Milestone:** M1

## Goal

Offer to create the conventional document layout from built-in templates,
without ever overwriting a file the person already has.

## Build

- `DocStore::scaffold() -> Result<ScaffoldReport, StoreError>` with
  `{ created: Vec<String>, skipped: Vec<String> }`.
- Create `sdlc/epics/`, `sdlc/decisions/`, `sdlc/features/`, and any missing core
  files. An epic's own `tickets/` directory is created when the epic is created,
  not at scaffold time.
  - `sdlc/PRD.md` — one H1, a short purpose section, and the exact feature
    index region:
    `<!-- sdlc:feature-index:start -->` … `<!-- sdlc:feature-index:end -->`.
  - `sdlc/architecture.md`, `sdlc/conventions.md`, `AGENTS.md` — short starter
    files with one H1 and obvious TODO sections.
- Use `Expectation::Missing` for every file; an existing file is skipped, not an
  error. Directories are created on demand.
- Templates live in `src/store/templates.rs` or as `include_str!` files under
  `src/store/templates/`.

## Already in place

- Atomic writes, templates, layout, number allocation (0011–0015).

## Acceptance criteria

- [ ] Empty project gets all core files and the three top-level directories.
- [ ] A second call creates nothing and reports the existing files as skipped.
- [ ] A pre-existing `sdlc/PRD.md` is byte-identical afterwards.
- [ ] A partial layout only fills the gaps.
- [ ] The PRD markers are exactly the strings 0028 will look for.
- [ ] Every starter file is valid Markdown with exactly one H1.
- [ ] Scaffold creates no `sdlc/tickets/` directory and no fake epic.

## Tests

- **Unit:** empty dir, second call, partial layout, existing custom PRD.
- **Unit:** marker constants match the strings used by the index writer.
- **Integration:** scaffold then `tree()`; all core entries appear.
- **Unit:** read-only project directory returns a clear IO error.

## Gotchas

- Never auto-scaffold; 0026 requires explicit consent.
- Empty directories are not tracked by git. The store must create them when
  needed and must not assume they exist.
- Keep starter text genuinely short; a bloated scaffold contradicts the
  product's purpose.
- Do not add `CLAUDE.md`; the PRD is explicit that `AGENTS.md` is the only
  rules file.
