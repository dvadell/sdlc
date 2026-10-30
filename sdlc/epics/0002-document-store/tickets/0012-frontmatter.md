---
status: todo
depends_on: ["0001"]
---

# 0012: YAML front matter parsing

**Epic:** [0002 Document store](../description.md) · **Depends on:** 0001 · **Milestone:** M1

## Goal

Read the small metadata block at the top of epics, features, decisions and
tickets without making the document unreadable when the YAML is wrong.

## Build

- Evaluate the maintained YAML options (`serde_yaml_ng`, `serde_norway`,
  `saphyr`, or whatever is current), pick one, and record the choice as ADR
  `0009` in `sdlc/decisions/`. Ticket 0059 verifies the nine ADRs, so
  coordinate the number rather than creating two files. Do not use archived
  `serde_yaml`.
- `src/store/frontmatter.rs`:
  - `split(content) -> (Option<&str>, &str)` for the leading `---` block.
  - Typed `DocMeta` with `status: Option<Status>`, `feature: Option<String>`,
    `depends_on: Vec<String>`, `supersedes: Option<u32>`. Keep numeric ids as
    strings here so quoted four-digit values survive; convert to numbers only
    after validation. Epic membership is not a front matter field.
  - Kind-specific status enums: epic description (`planned`, `in-progress`,
    `done`), feature (`planned`, `in-progress`, `shipped`), decision
    (`proposed`, `accepted`, `superseded`), ticket (`todo`, `in-progress`,
    `done`).
  - A small custom deserializer for `depends_on` accepts a quoted string or an
    integer, then normalizes ids to four digits so hand-written legacy metadata
    still parses. The conventions still recommend quoted values.
- Missing front matter, malformed YAML, unknown keys and invalid status values
  produce `None`/defaults plus a warning. They never make the document
  unreadable.
- The parser never rewrites the document; editing preserves the original text
  byte-for-byte.

## Already in place

- Document read (0010); conventions define the allowed fields.

## Acceptance criteria

- [ ] Valid front matter for all four kinds parses.
- [ ] No front matter → no status, no error.
- [ ] Malformed YAML → warning, document still listed and editable.
- [ ] `---` inside the body or a horizontal rule is not treated as the closing
  marker.
- [ ] An unknown status is treated as absent with a warning naming the file.
- [ ] The crate choice is recorded in a decision file.

## Tests

- **Unit:** each kind, absent block, empty block, closing marker missing.
- **Unit:** quoted values, comments, `depends_on: ["0004", "0005"]`, single
  value, and an unquoted legacy `depends_on: [4, 5]`.
- **Unit:** malformed YAML, wrong type (`status: 3`), unknown key, duplicate
  key.
- **Unit:** CRLF document and a BOM before `---`.
- **Unit:** parsing does not alter the input string.

## Gotchas

- `---` in Markdown is also a horizontal rule; only a block starting at byte 0
  (after an optional BOM) counts.
- A YAML parser can expand aliases; cap document size before parsing or disable
  aliases if the crate allows it.
- Keep unknown keys untouched but do not expose them in the API yet; the editor
  saves the raw content, so they survive.
- Status values are case-sensitive; do not silently lowercase them.
