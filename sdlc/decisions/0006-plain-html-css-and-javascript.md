---
status: accepted
---

# 0006: Plain HTML, CSS and JavaScript

## Context

The frontend must ship inside the binary as it is, without a build step (see
architecture sections 3.6 and 10).

## Decision

Ship plain HTML, CSS and JavaScript with no build step and two vendored
libraries.

## Consequences

- No framework, bundler or npm enters the project.
- Markdown rendering uses the vendored `marked` and sanitizes with `DOMPurify`.
