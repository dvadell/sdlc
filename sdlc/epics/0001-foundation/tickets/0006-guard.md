---
status: done
depends_on: ["0004"]
---

# 0006: Host and Origin guard middleware

**Epic:** [0001 Foundation and server shell](../description.md) · **Depends on:** 0004 · **Milestone:** M1

## Goal

Stop a malicious web page from driving the local server through the browser
(DNS rebinding and cross-site requests), without breaking the app's own
same-origin requests.

## Build

- `src/guard.rs` as axum middleware, before every route:
  - `Host` must equal `127.0.0.1:<port>` or `localhost:<port>` (the actual bound
    port). Missing or unparsable `Host` is rejected.
  - If `Origin` is present it must equal `http://127.0.0.1:<port>` or
    `http://localhost:<port>`. `https`, `null` and foreign origins are rejected.
  - Absent `Origin` is allowed for non-browser clients such as curl.
- State-changing methods (`POST`, `PUT`, `PATCH`, `DELETE`) must send
  `Content-Type: application/json` (parameters such as `charset` allowed).
  Empty JSON bodies are fine; return 415 otherwise.
- Rejections use `ApiError` (403 or 415) with the standard JSON shape.

## Already in place

- Router, actual bound port, `ApiError` (0003, 0004).

## Acceptance criteria

- [x] Same-origin `Host` and `Origin` pass.
- [x] `Host: evil.example`, `Host: 127.0.0.1:<wrong-port>` fail.
- [x] Foreign and `https` origins fail; absent origin passes.
- [x] `POST` with `text/plain` fails before reaching a handler.
- [x] `GET` without content type still passes.

## Tests

- **Integration:** each case above, asserting status and `error.code`.
- **Integration:** `localhost` and `127.0.0.1` both accepted.
- **Integration:** header values are case-insensitive where HTTP requires it.
- **Security:** two `Host` headers, `Origin: null`, `Origin: http://evil.test`.

## Gotchas

- The port in the allow-list must be the real bound port, not the requested
  `0`; pass it into the middleware state.
- Browsers omit `Origin` on some same-origin GETs but send it on POST; allow
  both exact forms.
- Do not trust `X-Forwarded-*`; there is no proxy in v1.
- `DELETE` with no body still needs the JSON content type from the UI; document
  that in `api.js` when it is written (0021).
- Keep the check O(1) and allocation-light; it runs on every request.
