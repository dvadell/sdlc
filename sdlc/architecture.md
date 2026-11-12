# sdlc: Architecture

*Status: Draft v0.1 · Describes how the system is built to satisfy `sdlc/PRD.md`. Library choices and code style are in `sdlc/conventions.md`. Individual choices and their reasons are recorded as decisions in `sdlc/decisions/`.*

## 1. Overview

`sdlc` is one Rust process that does three jobs:

1. **Serves a web UI** (embedded in the binary) on a loopback port.
2. **Manages documents**: reads, validates and writes the project's Markdown files.
3. **Brokers AI chat**: forwards chat requests to an OpenAI-compatible API and turns the AI's replies into reviewable proposals.

There is no database. The project's files are the only persistent state. The server holds a small amount of in-memory state (open proposals, file hashes) that is safe to lose on restart.

```
Browser (plain JavaScript app)
  ├─ editor + preview + diff view
  └─ chat panel
        │  HTTP/JSON, SSE
        ▼
┌──────────────────────── sdlc process (127.0.0.1:PORT) ─────────────────────────┐
│  Guard middleware (Host / Origin check)                                         │
│  ┌───────────┐  ┌───────────────┐  ┌──────────────┐  ┌───────────────────────┐  │
│  │ Static    │  │ Document API  │  │ Chat API     │  │ Events API (SSE)      │  │
│  │ assets    │  │               │  │              │  │ file-changed notices  │  │
│  └───────────┘  └──────┬────────┘  └──────┬───────┘  └──────────▲────────────┘  │
│                        │                  │                     │               │
│                 ┌──────▼────────┐  ┌──────▼───────┐      ┌──────┴───────┐       │
│                 │ DocStore      │  │ Chat service │      │ File watcher │       │
│                 │ (layout,      │  │ prompt build │      └──────▲───────┘       │
│                 │  validation,  │  │ stream parse │             │               │
│                 │  hashes)      │  └──────┬───────┘             │               │
│                 └──────┬────────┘         │                     │               │
│                        │           ┌──────▼───────┐             │               │
│                        │           │ Proposal     │             │               │
│                        │◄──────────┤ service      │             │               │
│                        │  apply    │ (diffs)      │             │               │
│                        │           └──────────────┘             │               │
└────────────────────────┼────────────────────────────────────────┼───────────────┘
                         ▼                                        │
                 Project directory (sdlc/, AGENTS.md)  ───────────┘

Chat service ──HTTPS──► OpenAI-compatible API (configured by env vars)
```

## 2. Runtime and configuration

| Setting | Source | Default |
|---------|--------|---------|
| Project directory | `--dir <path>` | `.` |
| Port | `--port <n>` | `9000` |
| Open browser on start | `--open` | off |
| AI base URL | `SDLC_AI_BASE_URL` | none (chat disabled if unset) |
| AI API key | `SDLC_AI_API_KEY` | none (optional, local servers often need none) |
| AI model | `SDLC_AI_MODEL` | none (chat disabled if unset) |
| PRD max bytes | `SDLC_PRD_MAX_BYTES` | `12000` |

- The listen address is always `127.0.0.1`; there is deliberately no flag to change it.
- Configuration is parsed once at startup into an immutable `Config` held in shared state.
- If the AI variables are missing, the server still starts. `GET /api/config` reports `ai_configured: false` and the UI explains how to configure it.
- The API key is never sent to the browser and never logged.

## 3. Components

### 3.1 Guard middleware
Runs before every route. Rejects the request unless:
- the `Host` header is `127.0.0.1:<port>` or `localhost:<port>`, and
- if an `Origin` header is present, it matches `http://127.0.0.1:<port>` or `http://localhost:<port>`.

This blocks DNS-rebinding and cross-site requests from malicious pages. State-changing routes also require `Content-Type: application/json`, which browsers will not send cross-origin without a preflight.

### 3.2 DocStore
The only component that touches the filesystem. Responsibilities:

- **Layout knowledge.** Defines which paths are valid documents:
  - `sdlc/PRD.md`, `sdlc/architecture.md`, `sdlc/conventions.md`, `AGENTS.md`
  - `sdlc/epics/<epic>/description.md`
  - `sdlc/epics/<epic>/tickets/<ticket>.md`
  - `sdlc/decisions/NNNN-*.md`
  - `sdlc/features/*.md`

  `<epic>` and `<ticket>` are single path segments with a four-digit prefix:
  `0001-foundation`, `0001-project-skeleton`.
- **Path safety.** Every path from the outside is parsed into a validated `DocPath` (relative, no `..`, matches the layout). The resolved path is checked not to escape the project root, including through symlinks.
- **Reads and writes.** Reads return content plus a content hash. Writes require the hash the client last saw; a mismatch returns a conflict instead of overwriting. Writes are atomic (temporary file in the same directory, then rename).
- **Listing.** Walks the allowed layout to answer `GET /api/tree`: the core files, each epic's `description.md` with its tickets, decisions and features, each entry carrying kind, path, title, status, epic, optional feature binding, ticket dependencies and the derived epic/feature orphan flags. Titles come from the first ATX H1 outside front matter and fenced code blocks, falling back to a humanized file name; the result is deterministic and files that fail validation or cannot be read are skipped, never failing the listing. Tickets are grouped under the epic directory that contains them, which the path states and the front matter never repeats, and a ticket whose epic directory has no `description.md` is derived as orphaned but still listed.
- **Numbering.** Allocates the next `NNNN` for epic directories, decisions and tickets. Ticket numbers are global across all epic `tickets/` directories; epic and ticket sequences are independent.
- **Metadata.** Parses the optional YAML front matter (see section 6) for status and optional feature links, and the typed ticket metadata for status, feature and dependencies. Epic membership is derived from the ticket path, not stored as a field; a ticket whose epic directory has no `description.md` is derived as orphaned, and a ticket naming a feature that no longer resolves is derived as feature-orphaned.
- **Feature index.** Rewrites the region of `sdlc/PRD.md` between the markers `<!-- sdlc:feature-index:start -->` and `<!-- sdlc:feature-index:end -->` from the feature files whenever a feature is created, renamed, deleted or changes status.
- **Scaffolding.** Creates the starter layout from built-in templates on request.
- **Creation from templates.** `DocStore::create` validates and slugifies the title, renders the kind's built-in template and writes through the missing-expectation path so existing documents are never overwritten. Numbered kinds allocate the next `NNNN`, and tickets land in their epic's `tickets/` directory with an optional existing feature binding.
- **Lifecycle.** `DocStore::rename` resolves both the source and the destination, refuses kinds that differ or numbers that change, and renames through `fs::rename`; an epic rename moves its whole directory, so its tickets move with it. `DocStore::delete` refuses core documents, refuses an epic that still holds tickets, and reports the tickets orphaned by a deleted feature.

### 3.3 File watcher
Watches the project's document paths and publishes `file-changed` events. Events caused by sdlc's own writes are suppressed by comparing content hashes with what DocStore just wrote. Events are debounced so editors that write in several steps produce one notification.

### 3.4 Chat service
Stateless: the browser sends the full conversation with each request.

1. Builds the **system prompt** (role, rules for proposals, the output format in section 5).
2. Adds the **document context**: the open document plus attached documents, each labelled with its path. Context size is not managed; documents are assumed to fit.
3. Appends the conversation messages.
4. Calls `POST {base_url}/chat/completions` with `stream: true`.
5. Relays text deltas to the browser as they arrive.
6. When the stream ends, scans the full reply for proposal blocks and emits one
   `warning` event per skipped block; when blocks are found, passes them to the
   Proposal service and emits a `proposal` event.

The AI has no tools and no function calling. This keeps the integration compatible with any OpenAI-compatible server.

### 3.5 Proposal service
Turns AI output into something reviewable and safe to apply.

- A **proposal** is a set of file changes: `create`, `modify` or `delete`, each with the new content.
- On creation, it reads the current content and hash of every affected file and computes a unified diff.
- Every proposed path is validated through DocStore. A proposal that touches an invalid path is rejected as a whole.
- Proposals live in memory with an id and expire when the server restarts.
- **Accept** applies all changes together, or none. Before applying, it re-checks the recorded hashes; if any file changed since the proposal was made, it fails with a conflict and the person must regenerate.
- **Reject** discards the proposal.
- Whole-proposal accept/reject only; there is no per-hunk logic.

### 3.6 Frontend
A single-page app written in plain HTML, CSS and JavaScript (ES modules, no framework, no bundler, no build step). The files are embedded in the binary as they are. Main areas:

- **Sidebar:** document tree grouped by type, epics with ticket counts, ticket groups under their epic, feature statuses, create/rename/delete actions.
- **Editor:** a plain `<textarea>` for Markdown source, with a preview toggle.
- **Diff view:** replaces the editor when a proposal is pending. It shows the unified diff computed by the server, one section per affected file, with added and removed lines colored, and accept-all / reject-all buttons.
- **Chat panel:** message list, input, attach-document control, and the built-in prompt shortcuts.
- **Onboarding:** when the project has no starter layout yet, the sidebar shows the empty state instead of a document tree, with one sentence, the path that would be created and a create-starter-documents action. Nothing is written until the person clicks the action, and a successful scaffold refreshes the tree and opens the PRD.

Rendered Markdown (both the person's documents and AI output) is converted with `marked` and sanitized with `DOMPurify` before insertion into the page. Both libraries are single files committed to the repository (`frontend/vendor/`). Chat uses `fetch` with a streamed response body, because `EventSource` only supports GET; the file-change events use `EventSource`.

## 4. HTTP API

All bodies are JSON unless noted. Errors use one shape: `{ "error": { "code": "...", "message": "..." } }`.

| Method and path | Purpose |
|-----------------|---------|
| `GET /api/config` | Project directory name, port, model name, `ai_configured` |
| `GET /api/tree` | All documents with type, title, status and epic/feature linkage |
| `GET /api/docs/{*path}` | Document content and hash |
| `PUT /api/docs/{*path}` | Save content; body includes the expected hash; 409 on mismatch |
| `POST /api/docs` | Create an epic, feature, decision or ticket |
| `POST /api/docs/move` | Rename a document |
| `DELETE /api/docs/{*path}` | Delete an epic (only when its `tickets/` is empty), feature, decision or ticket |
| `POST /api/scaffold` | Create the starter layout |
| `POST /api/chat` | Start a chat completion; response is an SSE stream |
| `POST /api/proposals/{id}/accept` | Apply a proposal |
| `POST /api/proposals/{id}/reject` | Discard a proposal |
| `GET /api/events` | SSE stream of `file-changed` events |
| `GET /*` | Embedded frontend assets, falling back to `index.html` |

### Chat stream events
`POST /api/chat` returns `text/event-stream` with these events:

| Event | Data |
|-------|------|
| `delta` | `{ "text": "..." }`, a chunk of the reply |
| `warning` | `{ "message": "..." }`, one per malformed or skipped block |
| `proposal` | `{ "id": "...", "changes": [ { "path", "action", "diff" } ], "warnings": [ ... ] }` |
| `error` | `{ "code": "...", "message": "..." }` |
| `done` | `{}` |

## 5. How the AI proposes changes

Because the AI has no tools, it proposes changes with a plain-text convention in its reply. The system prompt instructs the model to put each file change in a block:

```
=== FILE: sdlc/features/export.md ===
(the complete new content of the file)
=== END FILE ===

=== DELETE: sdlc/epics/0001-foundation/tickets/0007-old-ticket.md ===
```

Rules:
- A `FILE` block carries the **full new content**, not a patch. The server computes the diff, which is more reliable than asking a model to write correct patch syntax.
- A path that exists is a `modify`; one that does not is a `create`.
- Text outside blocks is ordinary chat and is shown as such.
- The parser ignores malformed or unterminated blocks and reports them in the chat as a warning rather than failing the stream.

## 6. Document conventions the system relies on

Documents are plain Markdown. The only sdlc-specific elements are:

1. **YAML front matter** (standard in Markdown tooling) on epics, features, decisions and tickets:
   - epic descriptions: `status: planned | in-progress | done`
   - features: `status: planned | in-progress | shipped`
   - decisions: `status: proposed | accepted | superseded`, optional `supersedes: NNNN`
   - tickets: optional `feature: <feature-file-name>`, `status: todo | in-progress | done`, optional `depends_on: ["NNNN"]`
2. **Feature index markers** in `sdlc/PRD.md` (HTML comments, invisible when rendered).

Files without front matter are still listed and editable; they just show no status. A ticket's epic is the directory that contains its `tickets/` folder, so there is no `epic` key to disagree with the path. A ticket whose parent epic has no `description.md` is listed as unassigned, not hidden.

## 7. Key flows

### Edit and save
1. UI loads `GET /api/docs/{path}` and keeps the returned hash.
2. On save, UI sends `PUT` with the new content and that hash.
3. DocStore compares the hash with the file on disk. Match: write atomically and return the new hash. Mismatch: 409, and the UI offers to reload or compare.
4. If the document was a feature, DocStore also refreshes the PRD feature index.

### Chat with a proposal
1. UI sends the conversation, the open document path and attached paths to `POST /api/chat`.
2. Chat service streams `delta` events while the model replies.
3. At the end, proposal blocks are parsed and handed to the Proposal service, which validates paths and computes diffs.
4. UI receives `proposal`, switches the main area to the diff view.
5. The person accepts: `POST /api/proposals/{id}/accept`. Hashes are re-checked, all files are written, the feature index is refreshed, and `done` state is shown. Rejection simply discards.

### External edit
1. Another tool changes a document on disk.
2. Watcher emits `file-changed` after the debounce.
3. If the document is open and unmodified, the UI reloads it. If it has unsaved changes, the UI asks the person to choose.

### Ticket generation
A built-in prompt sends the feature spec plus `conventions.md` to the model, asking for one `FILE` block per ticket under `sdlc/epics/<epic>/tickets/`. The person selects the epic before sending. The proposal writes each ticket into that epic's `tickets/` directory; membership comes from the path, and generated tickets also carry `feature: <slug>` when they came from a feature. Numbering is assigned by the Proposal service, not by the model, so tickets never collide. Regeneration includes the feature's existing tickets in the selected epic in the context; the diff shows what would change.

## 8. Error handling and failure modes

| Situation | Behaviour |
|-----------|-----------|
| AI not configured | Chat disabled with setup instructions; editing unaffected |
| AI endpoint unreachable or returns an error | `error` event in the stream; message shown in chat; nothing is changed |
| Stream interrupted | Partial text kept in chat; no proposal is created |
| Malformed proposal block | Ignored, with a visible warning |
| Ticket's parent epic has no `description.md` | Listed as unassigned with a warning; editing unaffected |
| Proposal touches an invalid path | Whole proposal rejected, reason shown |
| File changed since the proposal was made | Accept fails with a conflict; nothing is written |
| Write fails midway through a multi-file accept | Already-written files are reported; the person is told exactly which files changed |
| Port already in use | Process exits with a clear message suggesting `--port` |
| Project directory has no layout | UI offers to scaffold it |

## 9. Security

- Loopback bind plus Host/Origin checks (section 3.1).
- All file access goes through `DocPath` validation; only the known layout is reachable.
- The AI can only produce text. Its output is applied solely through an explicit person-approved proposal.
- Markdown rendering is sanitized to prevent script injection from documents or AI output.
- The API key lives only in the process environment.
- Document content is sent to the configured AI service. The UI shows the service host so this is never hidden.

## 10. Build and distribution

- The `frontend/` directory is embedded into the Rust binary at compile time exactly as it is. In debug builds, assets are read from disk, so editing a frontend file only needs a browser refresh.
- A release build is one `sdlc` executable per target platform.
- There is no Node, npm or frontend build step anywhere in the project.

## 11. Testing strategy

- **Unit tests** for `DocPath` validation (including nested epic tickets), front matter parsing, proposal block parsing, numbering, epic/ticket grouping and feature index generation.
- **Integration tests** that start the server against a temporary project directory and a mock OpenAI-compatible server, covering save conflicts, proposal accept/reject, and chat streaming.
- **Security tests** for traversal attempts, bad Host/Origin headers, and symlink escapes.
- **Frontend:** no automated tests in v1. Logic is kept in small functions (SSE frame parsing, diff line classification) so tests can be added later. A short manual checklist is run before each release.

## 12. Decisions to record

These choices are made in this document and recorded as decision files in `sdlc/decisions/`:

1. Use `axum` on `tokio` for the server: [0001](decisions/0001-use-axum-on-tokio.md).
2. Files as the only storage; no database: [0002](decisions/0002-files-as-the-only-storage.md).
3. Proposals via text blocks, not tool calling: [0003](decisions/0003-proposals-via-text-blocks-not-tool-calling.md).
4. Server computes diffs from full-file content: [0004](decisions/0004-server-computes-diffs-from-full-content.md).
5. Stateless chat endpoint; history lives in the browser: [0005](decisions/0005-stateless-chat-endpoint.md).
6. Plain HTML/CSS/JavaScript for the embedded frontend, with no build step and two vendored libraries: [0006](decisions/0006-plain-html-css-and-javascript.md).
7. Optimistic concurrency with content hashes instead of file locks: [0007](decisions/0007-optimistic-concurrency-with-content-hashes.md).
8. Epics as directories with a `description.md` and a `tickets/` subdirectory; a ticket's path determines its exact epic: [0008](decisions/0008-epics-as-directories-with-tickets.md).
9. The YAML front matter crate chosen in 0012 (recorded there as an ADR): [0009](decisions/0009-use-serde-yaml-ng.md).
