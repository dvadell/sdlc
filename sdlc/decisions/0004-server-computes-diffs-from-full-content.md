---
status: accepted
---

# 0004: Server computes diffs from full content

## Context

A model-written patch is unreliable; the person reviews diffs before anything
touches disk (see architecture sections 5 and 3.5).

## Decision

The server computes diffs from the full content of every affected file.

## Consequences

- The person reviews a real diff, not model-written patch syntax.
- Accepts re-check the recorded hashes and apply all changes together or none.
