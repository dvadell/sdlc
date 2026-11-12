# sdlc

`sdlc` is a local, file-based server for project documentation: it serves a
plain Markdown web UI, manages the project's documents and brokers AI chat.

The documents are plain Markdown files in the repository — the PRD, architecture,
conventions, decisions, epics, features, tickets and the AI rules file
(`AGENTS.md`) — for example
`sdlc/epics/0001-foundation/tickets/0001-project-skeleton.md`. See
[`sdlc/TICKETS.md`](sdlc/TICKETS.md) and the other docs under `sdlc/` for the full
layout.

## Build

```sh
just release
```

`just release` runs `cargo build --release`; the release binary is named `sdlc`
(`sdlc.exe` on Windows).

The release profile enables `lto = "thin"` and `strip = true` (platform allowing);
the trade-off is a longer release build and stripped debug information that can
hamper panic backtraces.

## Run

```sh
./target/release/sdlc --dir <path> --port 9000 --open
```

- `--dir <path>` — project directory to operate on; default `.`.
- `--port <n>` — listen port; default `9000`; `0` lets the OS pick an ephemeral port.
- `--open` — open the default browser after start; default off.

The server listens on `127.0.0.1` only and manages the documents of the project
directory it is pointed at.

A project without documents yet offers a scaffold action in the UI.

## Release

The release binary embeds the frontend (`frontend/`) into the binary, so it serves
the UI from an arbitrary working directory with no `frontend/` folder beside it.
Debug builds read the frontend from disk instead, so frontend edits during
development only need a browser refresh.

- `just check-release` builds the release binary, copies it to a fresh temp
  directory, starts it against an empty project on an ephemeral port and checks
  `GET /` and `GET /api/config`.

There is no Node/npm step at any point: the frontend is plain HTML, CSS and
JavaScript served as-is.

- `sdlc --version` prints the package version (`sdlc 0.1.0`).

Measured on Linux with the release profile: the binary is about 6.9 MB, startup to
the listening line is effectively instant (about 8 ms), and idle memory is about
6 MB.

Per platform: release binaries are built per target platform — `sdlc` on Linux
and macOS, `sdlc.exe` on Windows (for example `cargo build --release
--target x86_64-pc-windows-msvc`). The release check script runs on Linux and macOS
(bash); on Windows run `just release` and start `sdlc.exe` directly.

## Configuration

| Setting | Variable | Default |
|---------|----------|---------|
| Project directory | `--dir <path>` | `.` |
| Port | `--port <n>` | `9000` |
| Open browser on start | `--open` | off |
| AI base URL | `SDLC_AI_BASE_URL` | none (chat disabled if unset) |
| AI model | `SDLC_AI_MODEL` | none (chat disabled if unset) |
| AI API key | `SDLC_AI_API_KEY` | none (optional, local servers often need none) |
| PRD max bytes | `SDLC_PRD_MAX_BYTES` | `12000` |

The PRD max bytes variable is the advisory upper bound of the PRD size before
`sdlc` warns that the PRD grew too large; it is a positive integer of bytes with
an upper bound of `1000000`.

Point the AI at a local OpenAI-compatible server with:

```sh
export SDLC_AI_BASE_URL=http://localhost:11434/v1
export SDLC_AI_MODEL=llama3.2
```

`SDLC_AI_API_KEY` is optional and only needed by servers that require a bearer token.

## Privacy

Document content is sent to the AI host configured through `SDLC_AI_BASE_URL` (for
example `http://localhost:11434`, shown in the UI as the AI host). The API key stays
in the environment and is never sent to the browser. No chat history is stored.

## Security

- No auth. Local only.
- The server binds to `127.0.0.1` and rejects requests whose `Host` or `Origin`
  header does not match the local address.
- Only known document paths inside the project directory are reachable.

## Limitations (v1)

- No hosting, no network access beyond loopback, no authentication and no
  multi-user support.
- The AI reads project documents only; it has no access to source code, runs no
  commands and gets no tools.
- Context size is not managed; documents are assumed to fit.
- No chat history persistence, no git operations, no database, no WYSIWYG editor,
  no sync with external ticket trackers.

## Development

- `just check` runs the Rust format, lint and test tasks plus the frontend tests.
- `just check-release` builds the release binary and proves it is self-contained.
- The frontend is plain JavaScript with no build step; the vendored libraries live
  in `frontend/vendor/`. No Node/npm is needed.

## Further reading

- [`sdlc/PRD.md`](sdlc/PRD.md) — the what and why.
- [`sdlc/architecture.md`](sdlc/architecture.md) — the how.
- [`sdlc/conventions.md`](sdlc/conventions.md) — the how we write code.
