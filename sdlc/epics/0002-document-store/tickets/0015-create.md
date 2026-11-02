---
status: done
depends_on: ["0011", "0012", "0014"]
---

# 0015: Create documents from templates

**Epic:** [0002 Document store](../description.md) · **Depends on:** 0011, 0012, 0014 · **Milestone:** M1

## Goal

Create an epic, feature, decision or ticket file with valid front matter, a
title and a conventional name, in one safe operation.

## Build

- `src/store/templates.rs` with the epic description, feature, decision and
  ticket templates from `conventions.md` (epic, decision and ticket templates
  are normative there).
- `DocStore::create(&self, kind, title, epic: Option<&str>, feature: Option<&str>)`:
  - Validate a non-empty, single-line title (trimmed, ≤ 200 chars).
  - Slugify: lowercase ASCII, spaces to `-`, drop other punctuation, collapse
    `-`; reject if the result is empty.
  - Epic: allocate number, create the directory `sdlc/epics/NNNN-<slug>/` with
    `description.md` and an empty `tickets/` inside it, H1
    `# NNNN: <Title>`, front matter `status: planned`.
  - Feature: `sdlc/features/<slug>.md`, front matter `status: planned`.
  - Decision: allocate number, `sdlc/decisions/NNNN-<slug>.md`, H1
    `# NNNN: <Title>`, front matter `status: proposed`.
  - Ticket: require an existing epic whose `description.md` exists and, if
    given, an existing feature; allocate a global ticket number; write
    `sdlc/epics/<epic>/tickets/NNNN-<slug>.md`; front matter optional
    `feature: <slug>`, `status: todo`, `depends_on: []`; H1 numbered. Do not
    write an `epic` key.
- Create with `Expectation::Missing`; a duplicate feature returns a clear error.
- Create missing parent directories on demand, including `tickets/`.

## Already in place

- Write with expectations, numbering, front matter, layout (0011–0014).

## Acceptance criteria

- [x] Each kind produces a file whose front matter parses via 0012.
- [x] Epic creation produces `sdlc/epics/NNNN-slug/description.md`.
- [x] Epic H1 includes the allocated number; epic status defaults to `planned`.
- [x] Decision and ticket H1s include the allocated number.
- [x] A created ticket lands in the selected epic's `tickets/` directory and has
  no `epic` front matter key.
- [x] Duplicate feature names fail without touching the existing file.
- [x] Ticket creation without an epic, or with an unknown epic, fails with a
  useful message.
- [x] Ticket creation with an unknown feature fails; omitting the feature is
  allowed.
- [x] Titles with `/`, `..`, quotes, newlines or only punctuation are rejected
  or safely slugified; no path can escape.
- [x] Files end with exactly one trailing newline.

## Tests

- **Unit:** content for each kind; parse the produced front matter.
- **Unit:** slug cases (`Add CSV export`, `v2 API`, `!!!`, `../evil`,
  `Ünicode`).
- **Unit:** duplicate feature, missing/unknown epic, unknown ticket feature,
  title too long.
- **Unit:** ticket without a feature has no `feature` key and no `epic` key.
- **Unit:** the created ticket path is under the selected epic's `tickets/`
  directory.
- **Integration:** create then read; hashes match, no temp file remains.

## Gotchas

- Slug collisions are common; either fail with "a feature with this name exists"
  or append `-2`. Pick one and test it. Failing is simpler and predictable.
- Do not interpolate a raw title into YAML without escaping; the templates put
  the title in the H1 only, which avoids YAML quoting problems.
- A ticket's `feature` value is the slug without `.md` (matches the conventions
  example). Its epic value is not stored; the path decides it.
- Creating a directory is not atomic; a failure after `create_dir_all` is
  harmless.
