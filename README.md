# sdlc

`sdlc` is a local, file-based server for project documentation: it serves a
plain Markdown web UI, manages the project's documents and brokers AI chat.

## Run

```sh
cargo build --release
./target/release/sdlc --dir <path> --port 9000 --open
```

The server listens on `127.0.0.1` only and manages the documents of the project
directory it is pointed at.

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

## Privacy

Document content is sent to the AI host the server is configured with; the AI API
key stays in the environment and no chat history is stored.
