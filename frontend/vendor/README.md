# Vendored frontend libraries

Pinned single-file builds for the editor preview (ticket 0024). They live in this
folder and are served from the embedded asset route, so the app needs no CDN and
works offline once the binary is built.

| Library   | Version | Global      | Source                                 | License |
| --------- | ------- | ----------- | -------------------------------------- | ------- |
| marked    | 12.0.0  | `marked`    | <https://github.com/markedjs/marked>   | MIT     |
| DOMPurify | 3.4.17  | `DOMPurify` | <https://github.com/cure53/DOMPurify>  | Apache-2.0 / MIT |

## Loading

The builds are UMD bundles that expose globals, so they are loaded with classic
`<script>` tags before the ES module entry point:

```html
<script src="/vendor/marked.min.js"></script>
<script src="/vendor/purify.min.js"></script>
<script type="module" src="/js/main.js"></script>
```

## Rendering pipeline

The preview renders exactly:

```js
DOMPurify.sanitize(marked.parse(markdown))
```

then inserts the sanitized HTML into the preview element.

- `marked` turns Markdown into HTML but does not sanitize; the output is
  sanitized before it is inserted, never before.
- After sanitizing, links are forced to `target="_blank"` with
  `rel="noopener noreferrer"`. The sanitizer drops `target` in some
  configurations, so the attributes are set after sanitizing, not before.

## Notes

- Images with remote URLs will make network requests. That is acceptable in v1;
  the privacy statement lives in the README section of ticket 0058.
- Keep the pinned versions above; do not bump them without re-running the manual
  preview checks of ticket 0024.
