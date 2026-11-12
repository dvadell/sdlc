---
status: accepted
---

# 0009: Use serde_yaml_ng for YAML front matter

## Context

The document store reads the optional YAML front matter block of epics,
features, decisions and tickets (ticket 0012). The original `serde_yaml` crate is
archived, so a maintained serde-compatible YAML crate is needed before the store
can depend on one.

## Decision

Use `serde_yaml_ng` 0.10, the maintained fork of the archived `serde_yaml`, with
serde derive for the front matter structs.

## Alternatives considered

- `serde_yaml`, the original crate: rejected, it is archived and therefore no
  longer maintained.
- The other maintained forks evaluated at the time, `serde_norway` and `saphyr`:
  considered and not chosen.

## Consequences

- The archived `serde_yaml` is not used; `serde_yaml_ng` is the single YAML
  dependency of the project.
- The fork implements YAML 1.1, so unusual or very old front matter may not
  parse; those cases fall back to default metadata with a warning instead of
  making the document unreadable.
- The parser may expand YAML aliases; documents are small, but keep front matter
  blocks short.
- The choice is recorded here and reviewed by ticket 0059 instead of adding a
  second decision for the same decision later.
