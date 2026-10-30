---
status: todo
depends_on: ["0002", "0003"]
---

# 0004: Loopback server startup

**Epic:** [0001 Foundation and server shell](../description.md) · **Depends on:** 0002, 0003 · **Milestone:** M1

## Goal

Start an axum server bound to `127.0.0.1` on the configured port, print a URL
the person can click, and fail clearly when the port is taken.

## Build

- `src/main.rs`: parse `Config`, `init_tracing()`, bind a `tokio::net::TcpListener`
  to `127.0.0.1:<port>`, build the router, serve with Ctrl-C graceful shutdown.
- `src/lib.rs`: `pub async fn serve(config: Config, listener: TcpListener) -> ...`
  returning the actual bound address, so tests can bind port `0`.
- Print `sdlc listening on http://127.0.0.1:<actual>` and
  `AI: configured (<host>)` or `AI: not configured`.
- Map `AddrInUse` to a message suggesting `--port <other>`; exit status 1.
- `--open` launches the default browser after the listener is bound; a failure
  is a warning, never fatal. Use one small crate or the platform command, but do
  not block startup.
- Placeholder `GET /` returning `200 text/plain "sdlc"` until 0005.

## Already in place

- `Config` (0002) and `ApiError`/logging (0003).

## Acceptance criteria

- [ ] `--port 0` logs the real port, not `0`.
- [ ] The process binds only `127.0.0.1`.
- [ ] Occupied port prints one clear line and exits non-zero.
- [ ] The AI key never appears in startup output.
- [ ] Ctrl-C shuts down without panic.

## Tests

- **Integration:** bind a `TcpListener` on an ephemeral port, call `serve` on the
  same port, assert an `AddrInUse` error before serving.
- **Integration:** start on port `0`, read the returned address, request `/`,
  assert `200`.
- **Manual:** run twice on the same fixed port and read the message; try
  `--open` on the target OS.

## Gotchas

- Print the URL only after a successful bind, otherwise the person clicks a
  dead port.
- `--port 0` is intentionally allowed for tests; do not add a minimum port
  check.
- Passing the listener into `serve` avoids a test race between probing a port
  and binding it.
- Do not log the whole `Config` (0002 redacts, but avoid it anyway).
