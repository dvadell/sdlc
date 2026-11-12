# sdlc: Conventions

*Status: Draft v0.1 · The "how we write code" document. For what the product does see `sdlc/PRD.md`; for how the system is structured see `sdlc/architecture.md`.*

## 1. Stack

| Area | Choice | Notes |
|------|--------|-------|
| Language | Rust, edition 2024, stable toolchain | Pin `rust-version` in `Cargo.toml` and `rust-toolchain.toml` |
| Async runtime | `tokio` | Multi-threaded runtime |
| HTTP server | `axum` 0.8 | Path wildcards are written `/{*path}` |
| HTTP client | `reqwest` (rustls, `stream` feature) | For the AI API |
| Upstream SSE parsing | `eventsource-stream` | Parses the AI provider's stream |
| CLI | `clap` (derive) | Flags and env var handling |
| Serialization | `serde`, `serde_json` | |
| Front matter | `serde_yaml_ng` | Managed YAML with serde support; the archived `serde_yaml` is not used (ADR 0009) |
| Diffs | `similar` | Unified diffs for proposals |
| Hashing | `sha2` | Content hashes for conflict detection |
| File watching | `notify` with a debouncer | |
| Embedded assets | `rust-embed` | Reads from disk in debug builds |
| Errors | `thiserror` (libraries/modules), `anyhow` (only in `main`) | |
| Logging | `tracing`, `tracing-subscriber` | |
| Tests | built-in test harness, `tempfile`, `wiremock` | `wiremock` fakes the AI server |
| Frontend | Plain HTML, CSS and JavaScript (ES modules) | No framework, no build step, no npm |
| Markdown preview | `marked` + `DOMPurify` | Vendored single files in `frontend/vendor/`; always sanitize |

Add a dependency only when it removes real work. Prefer the standard library for small things.

## 2. Project layout

```
sdlc/
  Cargo.toml
  rust-toolchain.toml
  justfile                  # build, test, lint, release tasks
  src/
    main.rs                 # parse args, build state, start server
    config.rs               # Config from flags + env
    error.rs                # ApiError and its HTTP mapping
    guard.rs                # Host/Origin middleware
    state.rs                # AppState (shared, cheap to clone)
    store/
      mod.rs                # DocStore
      path.rs               # DocPath validation
      layout.rs             # what paths are valid documents
      frontmatter.rs
      index.rs              # PRD feature index rewriting
      scaffold.rs           # starter layout scaffolding
      templates.rs          # starter files
    watch.rs                # file watcher + event hub
    chat/
      mod.rs                # chat service
      client.rs             # OpenAI-compatible client
      prompt.rs             # system prompt and context building
      blocks.rs             # FILE/DELETE block parser
    proposal.rs             # proposals: build, diff, accept, reject
    routes/
      mod.rs
      docs.rs
      chat.rs
      proposals.rs
      events.rs
      assets.rs
  tests/                    # integration tests
  frontend/                 # embedded by rust-embed as-is; no build step
    index.html
    style.css
    js/                     # ES modules (see section 11)
    vendor/                 # marked.min.js, purify.min.js
  sdlc/                     # product documentation root
  AGENTS.md
```

One concern per module. Route modules contain only HTTP glue; logic lives in the store, chat and proposal modules so it can be tested without HTTP.

## 3. Rust style

### Tooling (required before every commit)
```
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test
```
The `justfile` provides `just check` which runs all three.

### Rules
- No `unwrap()` or `expect()` in non-test code, except for invariants established at startup, where `expect("reason")` must state the reason.
- No `panic!` in request paths.
- Prefer returning `Result` with a specific error type over `Box<dyn Error>`.
- No blocking filesystem or CPU-heavy calls inside async handlers. Use `tokio::fs`, or `tokio::task::spawn_blocking` for heavy work (diffing very large files, hashing).
- Don't hold a lock across an `.await`.
- Make invalid states unrepresentable: parse raw strings into types (`DocPath`, `DocKind`, `Status`) at the edge and pass the types inward.
- Public items get doc comments. Comments explain *why*, not what.
- Keep functions short enough to read without scrolling. Split when a function does more than one thing.
- Naming: standard Rust (`snake_case` functions, `CamelCase` types). Avoid abbreviations except common ones (`doc`, `cfg`, `req`).
- Modules re-export only what other modules need.

### Shared state
`AppState` is a struct of `Arc`-wrapped services, cloned into handlers by axum. It holds no mutable fields directly; services own their interior mutability.

```rust
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub store: Arc<DocStore>,
    pub proposals: Arc<ProposalService>,
    pub chat: Arc<ChatService>,
    pub events: EventHub,
}
```

## 4. Error handling

Each module defines its own error enum with `thiserror`. `ApiError` is the only type that knows about HTTP; it converts from module errors and picks the status code and a stable machine-readable `code`.

```rust
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("not an allowed document path: {0}")]
    NotAllowed(String),
    #[error("document not found: {0}")]
    NotFound(String),
    #[error("document changed on disk")]
    Conflict { current_hash: String },
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: String,
}

impl From<StoreError> for ApiError {
    fn from(err: StoreError) -> Self {
        let (status, code) = match &err {
            StoreError::NotAllowed(_) => (StatusCode::BAD_REQUEST, "not_allowed"),
            StoreError::NotFound(_) => (StatusCode::NOT_FOUND, "not_found"),
            StoreError::Conflict { .. } => (StatusCode::CONFLICT, "conflict"),
            StoreError::Io(_) => (StatusCode::INTERNAL_SERVER_ERROR, "io"),
        };
        ApiError { status, code, message: err.to_string() }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = Json(json!({ "error": { "code": self.code, "message": self.message } }));
        (self.status, body).into_response()
    }
}
```

Rules:
- Log the detailed cause with `tracing` at the point of conversion; show the person a short message.
- Internal errors (`io`) must not leak absolute filesystem paths to the client.
- Never include the API key or full AI request bodies in logs or errors.

## 5. Validated paths

All external path strings become a `DocPath` before use. Nothing else in the codebase builds file paths from request data.

```rust
/// A relative path that is known to be a valid sdlc document.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DocPath(PathBuf);

impl DocPath {
    pub fn parse(raw: &str) -> Result<Self, StoreError> {
        let path = Path::new(raw);
        let only_normal = path
            .components()
            .all(|c| matches!(c, Component::Normal(_)));

        if raw.is_empty() || raw.contains('\\') || !only_normal || !layout::is_valid(path) {
            return Err(StoreError::NotAllowed(raw.to_owned()));
        }
        Ok(Self(path.to_path_buf()))
    }

    pub fn as_path(&self) -> &Path {
        &self.0
    }
}
```

`DocStore` additionally canonicalizes the joined path and checks it still starts with the canonical project root, so symlinks cannot escape it.

## 6. Handlers

Handlers are thin: extract, call a service, map the result. They never touch the filesystem directly.

```rust
pub async fn get_doc(
    State(app): State<AppState>,
    Path(raw): Path<String>,
) -> Result<Json<DocResponse>, ApiError> {
    let path = DocPath::parse(&raw)?;
    let doc = app.store.read(&path).await?;
    Ok(Json(DocResponse::from(doc)))
}

pub async fn put_doc(
    State(app): State<AppState>,
    Path(raw): Path<String>,
    Json(body): Json<SaveRequest>,
) -> Result<Json<SaveResponse>, ApiError> {
    let path = DocPath::parse(&raw)?;
    let new_hash = app.store.write(&path, &body.content, &body.expected_hash).await?;
    Ok(Json(SaveResponse { hash: new_hash }))
}
```

Route registration (axum 0.8 syntax):

```rust
Router::new()
    .route("/api/docs/{*path}", get(get_doc).put(put_doc).delete(delete_doc))
    .route("/api/chat", post(chat))
    .layer(middleware::from_fn_with_state(state.clone(), guard::check))
    .with_state(state)
```

## 7. Streaming

The chat route returns an SSE response built from a stream of typed events. The chat service yields a Rust enum; the route converts it to SSE frames.

```rust
pub enum ChatEvent {
    Delta(String),
    Proposal(ProposalView),
    Error { code: &'static str, message: String },
    Done,
}

impl ChatEvent {
    pub fn into_sse(self) -> Event {
        match self {
            ChatEvent::Delta(text) => Event::default().event("delta").json_data(json!({ "text": text })),
            ChatEvent::Proposal(p) => Event::default().event("proposal").json_data(p),
            ChatEvent::Error { code, message } => {
                Event::default().event("error").json_data(json!({ "code": code, "message": message }))
            }
            ChatEvent::Done => Event::default().event("done").json_data(json!({})),
        }
        .unwrap_or_else(|_| Event::default().event("error"))
    }
}
```

Rules:
- The stream always ends with either `done` or `error`.
- Accumulate the full reply while streaming so proposal blocks can be parsed at the end.
- If the browser disconnects, drop the upstream request promptly.

## 8. Configuration

Flags (via `clap`):

| Flag | Default | Meaning |
|------|---------|---------|
| `--dir <path>` | `.` | Project directory |
| `--port <n>` | `9000` | Listen port on `127.0.0.1` |
| `--open` | off | Open the browser after start |

Environment variables:

| Variable | Required | Meaning |
|----------|----------|---------|
| `SDLC_AI_BASE_URL` | for chat | Base URL ending before `/chat/completions`, for example `https://api.openai.com/v1` or `http://localhost:11434/v1` |
| `SDLC_AI_MODEL` | for chat | Model name sent in each request |
| `SDLC_AI_API_KEY` | no | Sent as a bearer token when set |
| `SDLC_PRD_MAX_BYTES` | no | Maximum PRD size in bytes before a size warning; default 12000, upper bound 1000000 |

Print the listening URL and whether the AI is configured at startup, never the key.

## 9. Document conventions (for the files sdlc manages)

- Plain CommonMark Markdown; no sdlc-specific syntax beyond front matter and the PRD feature index markers.
- Filenames and directory names are lowercase kebab-case. Epics, decisions and tickets are prefixed with a four-digit number: an epic directory is `0001-foundation/`, and a decision or ticket file is `0003-use-axum.md`.
- An epic is a directory `sdlc/epics/<epic>/` containing `description.md` and `tickets/`. Its tickets are `sdlc/epics/<epic>/tickets/<ticket>.md`.
- Front matter keys are lowercase; allowed values are listed in `sdlc/architecture.md` section 6. Quote numbered references to keep leading zeros: `depends_on: ["0004"]`.
- Epic membership comes from the ticket path. Do not add an `epic` front matter key to a ticket; it could disagree with the directory. A ticket may name its source feature with `feature: <slug>`.
- Each document has exactly one H1 title.
- The PRD stays short. Library names and code do not go in the PRD.
- One fact lives in one place. Link to it elsewhere.

### Epic description template
```markdown
---
status: planned
---
# 0001: Foundation and server shell

## Outcome
What this epic delivers when it is done.

## Tickets
- [0001: Project skeleton](tickets/0001-project-skeleton.md)

## Exit criteria
Observable conditions for calling the epic done.
```

### Decision file template
```markdown
---
status: accepted
---
# 0003: Use axum

## Context
Why this decision was needed.

## Decision
What was chosen.

## Consequences
What gets easier, what gets harder.
```

### Ticket file template
Place at `sdlc/epics/<epic>/tickets/<ticket>.md`.
```markdown
---
feature: export
status: todo
depends_on: ["0004"]
---
# 0007: Add CSV export endpoint

## Description
What to build and why, in two or three sentences.

## Acceptance criteria
- Observable, testable statements.
```

## 10. Testing

- **Unit tests** live next to the code in `#[cfg(test)] mod tests`. Cover parsing, validation, numbering, front matter, epic/ticket grouping, index generation, proposal block parsing.
- **Integration tests** live in `tests/`. Each test creates a `tempfile::TempDir` project, starts the app on a random port, and talks to it over HTTP.
- Fake the AI with `wiremock`, returning a canned streaming response. Never call a real AI service in tests.
- Every bug fix gets a regression test.
- Security-sensitive code (`DocPath`, guard, symlink handling) has explicit negative tests: `..`, absolute paths, backslashes, encoded traversal, symlink escape, wrong Host, foreign Origin.
- Test names state behaviour: `save_with_stale_hash_returns_conflict`.

```rust
#[tokio::test]
async fn save_with_stale_hash_returns_conflict() {
    let app = TestApp::start().await;
    app.write_file("sdlc/PRD.md", "# PRD\n");

    let res = app.put("/api/docs/sdlc/PRD.md", json!({
        "content": "# New\n",
        "expected_hash": "not-the-current-hash",
    })).await;

    assert_eq!(res.status(), StatusCode::CONFLICT);
}
```

## 11. Frontend conventions

The frontend is plain HTML, CSS and JavaScript. There is no framework, no bundler, no TypeScript and no npm. The goal is a tool that works and is easy to change, not a polished UI.

- Load code with `<script type="module">`; split it into small ES module files.
- Use `const` and `let`, `async`/`await`, and `fetch`. Nothing that needs a transpiler.
- Keep one plain `state` object. Each area of the page has a `render...()` function that rebuilds that area with `replaceChildren()`. After changing state, call the relevant render function.
- Put text into the page with `textContent`. The only place HTML is inserted is the preview, and only through `DOMPurify.sanitize(marked.parse(markdown))`.
- The UI never assumes a save worked. It updates its stored hash only from the server's response.
- Handle the three chat stream outcomes explicitly: `done`, `error`, and connection loss.
- Keyboard basics: Ctrl/Cmd+S saves, Enter sends chat, Shift+Enter inserts a newline.
- Styling is one `style.css` file. Plain, readable, no framework.
- Optional: add JSDoc type comments on functions for editor hints. Not required.

Files:

| File | Responsibility |
|------|----------------|
| `index.html` | Page skeleton: sidebar, editor area, chat panel |
| `style.css` | All styles |
| `js/main.js` | Startup, wiring, keyboard shortcuts |
| `js/api.js` | `fetch` wrappers for every API route |
| `js/sse.js` | The SSE reader for POST bodies: the `readSse()` async generator over a fetch body stream |
| `js/events.js` | The SSE client for file changes: the `EventSource("/api/events")` connection, the clean reload, the dirty conflict banner and the silent tree refresh |
| `js/state.js` | The shared `state` object |
| `js/sidebar.js` | Document tree and create/rename/delete actions |
| `js/editor.js` | Textarea, save, preview toggle |
| `js/diff.js` | Rendering a proposal |
| `js/chat.js` | Chat panel, composer, attachments and prompt wiring |
| `js/prompts.js` | The built-in prompt shortcuts and their prompt texts |
| `vendor/marked.min.js`, `vendor/purify.min.js` | Pinned third-party files; note the version in a comment at the top of each file or in `vendor/README.md` |

### Reading the chat stream
`EventSource` cannot send a POST body, so the chat response is read manually:

```js
export async function* readSse(response) {
  const reader = response.body.getReader();
  const decoder = new TextDecoder();
  let buffer = "";
  while (true) {
    const { value, done } = await reader.read();
    if (done) break;
    buffer += decoder.decode(value, { stream: true });
    let end;
    while ((end = buffer.indexOf("\n\n")) !== -1) {
      const frame = buffer.slice(0, end);
      buffer = buffer.slice(end + 2);
      yield parseFrame(frame);
    }
  }
}

function parseFrame(frame) {
  let event = "message";
  let data = "";
  for (const line of frame.split("\n")) {
    if (line.startsWith("event: ")) event = line.slice(7);
    else if (line.startsWith("data: ")) data += line.slice(6);
  }
  return { event, data: JSON.parse(data) };
}
```

### Rendering a diff
The server sends a unified diff as text. The UI only colors the lines:

```js
export function renderDiff(container, diffText) {
  container.replaceChildren();
  for (const line of diffText.split("\n")) {
    const el = document.createElement("div");
    el.className = line.startsWith("+") ? "add" : line.startsWith("-") ? "del" : "ctx";
    el.textContent = line;
    container.append(el);
  }
}
```

## 12. Build, run, release

`justfile` tasks:

| Task | What it does |
|------|--------------|
| `just run` | `cargo run`; in debug builds frontend files are read from disk, so a browser refresh picks up changes |
| `just check` | `cargo fmt`, `cargo clippy`, `cargo test` |
| `just release` | `cargo build --release` (the frontend is embedded as-is) |
| `just check-release` | Build the release binary, copy it to a temp dir and prove it serves the embedded frontend |

- Release binaries are built per target platform and named `sdlc` (`sdlc.exe` on Windows).
- To update `marked` or `DOMPurify`, replace the file in `frontend/vendor/`, update the recorded version, and run through the manual checklist (open, edit, preview, chat, accept a proposal).

## 13. Git and commits

The person owns git. When the AI works on the code base:
- Do not commit, push, or create branches.
- Keep each change small and focused on one ticket.
- Mention in the summary which tests were added or changed.

## 14. Working with AI on this project

- Give the AI the relevant ticket, the feature spec, and this document. The full PRD is rarely needed.
- Ask for tests together with the code.
- Review diffs for new dependencies, `unwrap()` calls, and anything that builds paths from request data.
- If a convention here is wrong or missing, change this file first, then the code.
