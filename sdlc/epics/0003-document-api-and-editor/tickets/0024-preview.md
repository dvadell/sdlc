---
status: done
depends_on: ["0021"]
---

# 0024: Preview toggle with vendored libraries

**Epic:** [0003 Document API and editor](../description.md) · **Depends on:** 0021 · **Milestone:** M1

## Goal

Render the current Markdown safely in a toggleable preview, with no build step
and no CDN dependency.

## Build

- Vendor pinned single-file builds: `frontend/vendor/marked.min.js` and
  `frontend/vendor/purify.min.js`; add `frontend/vendor/README.md` with
  versions, source URLs and licenses.
- Load them before the module (classic `<script>` tags exposing globals) or as
  ES modules if the builds support it; pick one and document it.
- `togglePreview()` in `editor.js`: switches source/preview in the main area;
  when preview is visible, render on toggle and on a 150 ms debounce after
  input.
- Render exactly `DOMPurify.sanitize(marked.parse(markdown))`, then insert into
  a dedicated preview element.
- After sanitizing, force links to `target="_blank"` and
  `rel="noopener noreferrer"`.
- Preview button state is visible; returning to source restores the textarea
  and caret.

## Already in place

- Editor, shell, embedded assets.

## Acceptance criteria

- [x] Toggle shows rendered headings, lists, links, tables and fenced code.
- [x] `<script>`, `<img onerror>`, `<iframe>` and `javascript:` links are
  removed or neutralized.
- [x] Raw HTML in documents is escaped or sanitized, never executed.
- [x] Toggle back to source preserves text and does not mark the document dirty.
- [x] Preview updates while typing when already visible.
- [x] Works offline; no requests leave the origin for the libraries.

## Tests

- **Manual:** XSS payloads in a scratch document: `<script>alert(1)</script>`,
  `<img src=x onerror=alert(1)>`, `[x](javascript:alert(1))`,
  `<svg onload=alert(1)>`.
- **Manual:** markdown tables, nested lists, code fences and a very long line.
- **Manual:** links open a new tab with no opener.
- **Manual:** preview a document with `---` lines used as both front matter and
  a horizontal rule; nothing is executed and rendering stays sane.

## Gotchas

- `marked` does not sanitize; the order matters. Never insert `marked.parse`
  output before sanitizing.
- DOMPurify removes `target` by default in some configurations; set link
  attributes after sanitizing, not before.
- Images with remote URLs will make network requests. That is acceptable in v1
  but note it in the privacy section of 0058.
- Vendored files must be committed; do not fetch at runtime.
- Preview scroll position and selection are not preserved on re-render; that is
  acceptable, but returning to source must restore the caret.
