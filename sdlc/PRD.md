# sdlc: Product Requirements Document

*Status: Draft v0.3 · Scope: this document is the high-level "what and why" only. Stack, architecture and code conventions live in their own docs.*

## 1. Overview

**sdlc** is a single-binary, local web app for writing and maintaining the documents that drive AI-assisted software development: the PRD, architecture, conventions, decisions, epics, feature specs, tickets and the AI rules file (`AGENTS.md`). It pairs a document editor with an AI chat panel, so the person can think, draft and refine these documents together with an AI model.

Running `sdlc` starts a local server (default `http://127.0.0.1:9000`) for a project directory (default: the current directory). The documents remain plain Markdown files in the project's repository.

## 2. Problem

When building software with an AI model, the PRD tends to grow with every feature until it mixes product intent, architecture, library choices and code style. This makes it hard to maintain and wastes AI context. The fix is a set of small, focused documents, but managing them by hand in a text editor is tedious, and the AI is not integrated into the editing workflow.

## 3. Goals

1. Make it easy to keep project documents **small, separated by concern, and consistent**.
2. Let the person **edit documents and talk to an AI about them** in one place.
3. Keep everything as **plain files in the repo**, so other tools (Claude Code, Cursor, git) work with the same source of truth.
4. Be trivial to run: **one binary, one command, and a few environment variables** to point it at an AI service.

## 4. Non-goals (v1)

- Multi-user collaboration, accounts or authentication.
- Hosting or access over a network (local only).
- Reading or generating application source code. The AI is an aide for working on the specification only.
- Committing to git or any other version-control operation. The person handles that.
- Syncing with external ticket trackers.
- Persisting chat history.
- Managing model context limits (truncation, summarization, warnings).
- A database. Files are the storage.
- Rich WYSIWYG editing. Markdown source editing with preview is enough.

## 5. Users

A solo developer (primary) who writes software with the help of an AI model and wants structured, lean project documentation. Comfortable with a terminal and git.

## 6. Document model

sdlc manages a fixed, conventional layout inside the project directory:

```
sdlc/
  PRD.md
  architecture.md
  conventions.md
  epics/
    <epic>/
      description.md
      tickets/<ticket>.md
  decisions/NNNN-title.md
  features/name.md
AGENTS.md
```

`<epic>` is `NNNN-kebab-name` (for example `0001-foundation`). `<ticket>` is also `NNNN-kebab-name` (for example `0001-project-skeleton`).

- The **PRD** is short and contains vision, goals, non-goals and a **feature index** (one line per feature: link + status).
- **Epics** are directories, numbered sequentially. Each has a `description.md` with status (`planned`, `in-progress` or `done`) and a `tickets/` directory. An epic groups its tickets.
- **Features** and **decisions** are one file each. Decisions are numbered sequentially.
- **Tickets** are stored under their epic at `sdlc/epics/<epic>/tickets/<ticket>.md`, as **one Markdown file per ticket**. The path is the source of truth for epic membership: each ticket belongs to exactly one epic. A ticket may also name the feature it came from. Ticket numbers are global and sequential across all epics.
- **AGENTS.md** is the single AI rules file. sdlc does not manage `CLAUDE.md`; tools that expect it can point to `AGENTS.md`.
- If the layout does not exist, sdlc offers to create it with starter templates.

## 7. Core features

| ID | Feature | Summary | Status |
|----|---------|---------|--------|
| F1 | Document editor | Edit Markdown with live preview and save to disk | Planned |
| F2 | Document navigator | Sidebar listing PRD, architecture, conventions, rules, epics, features, decisions and tickets | Planned |
| F3 | Feature management | Create, edit, delete feature specs; set status; auto-maintain the PRD feature index | Planned |
| F4 | Decision management | Create, edit, delete ADRs with auto-numbering and status (proposed, accepted, superseded) | Planned |
| F5 | AI rules editing | Edit `AGENTS.md`, with AI help to keep it short and in sync with the other docs | Planned |
| F6 | AI chat panel | Side panel chat with an AI that sees the project documents | Planned |
| F7 | AI-proposed edits | AI suggests changes as a diff; the person accepts or rejects | Planned |
| F8 | Single-binary distribution | Frontend embedded; `sdlc` starts the local server (default port 9000) | Planned |
| F9 | Ticket generation | Break a feature spec into one file per ticket under an epic, reviewed as a diff before saving | Planned |
| F10 | Pluggable AI backend | Connect to any OpenAI-compatible API, configured by environment variables | Planned |
| F11 | Epic management | Create, edit and delete numbered epics; group tickets under exactly one epic | Planned |

## 8. Functional requirements

### Launch and configuration
- `sdlc` serves the project directory given by a flag (for example `--dir <path>`), defaulting to `.`.
- The port is set by a flag (for example `--port <n>`), defaulting to 9000. Any port is allowed.
- The AI backend is any **OpenAI-compatible API**. Environment variables set the base URL, API key and model. Exact variable names are defined in `conventions.md`.
- If no AI backend is configured, the editor works fully and the chat panel explains how to configure one.

### Editor and navigation
- Main area shows the selected document in an editor, with a toggle for rendered preview.
- Sidebar groups documents by type, shows epics with their ticket counts, features with their status, and groups tickets under their epic.
- Create, rename and delete for epics, features, decisions and tickets; delete requires confirmation. Deleting an epic is refused while its `tickets/` directory contains tickets; delete or move those tickets first.
- PRD, architecture, conventions and `AGENTS.md` can be edited but not deleted from the UI.
- Unsaved changes are indicated; saving writes the file immediately.
- Detect external changes to files (for example from another tool) and prompt to reload rather than overwrite silently.

### AI chat
- Chat panel is always visible beside the editor. History lasts for the browser session only and is not saved to disk.
- The AI sees only the project documents (the layout in section 6). It has no access to source code, the filesystem outside those documents, or any tools. It never runs commands.
- The open document is always included in the context. The person can attach other documents.
- AI replies stream in as they are generated.
- The AI never writes to disk directly. Changes are shown as a **diff** in the editor. The person accepts or rejects the **whole proposal**; there is no per-hunk acceptance in v1.
- Context size is not managed in v1: the documents are assumed to fit in the model's context window.
- Built-in prompts for common jobs: *split this PRD by concern*, *draft a feature spec from this idea*, *draft an ADR for this choice*, *trim AGENTS.md*, *check docs for contradictions or duplication*, *generate tickets from this feature*.

### Ticket generation
- From a feature spec, the person chooses the epic for the work; the AI drafts a set of tickets, each with a title, description, acceptance criteria and dependencies on other tickets.
- The proposal (new ticket files) is shown as a diff and saved to `sdlc/epics/<epic>/tickets/` on acceptance, one file per ticket. The ticket's path determines its epic; when generated from a feature, it also names that feature.
- Regenerating tickets for a feature shows a diff against that feature's existing ticket files in the chosen epic, so manual edits are not lost.
- Tickets can be created, edited and deleted by hand like any other document; a hand-created ticket must be created inside an epic.

### Consistency helpers
- Warn when the PRD exceeds a configurable size, or contains content better placed elsewhere (libraries, code blocks).
- Keep the PRD feature index in sync when features are added, renamed, deleted or change status.
- Report tickets whose parent epic has no `description.md` as unassigned instead of hiding them.

## 9. Constraints

- Implemented in **Rust**, shipped as one executable named `sdlc`.
- Listens on **port 9000 by default**, configurable with a CLI flag, bound to `127.0.0.1` only.
- No authentication, but requests must be checked against allowed `Host` and `Origin` values so a malicious web page cannot drive the local server from the browser.
- Operates only on files inside the project directory; path traversal must be impossible.
- The API key is supplied through the environment, never stored in project files.
- Document content is sent to whichever AI service is configured. This must be stated clearly in the UI and README, and is a reason to support local OpenAI-compatible servers.

*Specific crates, frontend approach and code patterns belong in `conventions.md` and `architecture.md`, not here.*

## 10. Success criteria

- From download to first edit takes under two minutes.
- A PRD, architecture and conventions set can be split out of a single bloated PRD in one chat session.
- A feature spec can be turned into a usable ticket list grouped under an epic in one chat session.
- Files written by sdlc are clean Markdown that other tools can read with no sdlc-specific syntax.
- Startup is effectively instant and idle memory use is small.

## 11. Milestones

1. **M1: Editor.** Binary serves UI; navigate, edit and save the fixed documents (F1, F2, F8).
2. **M2: Features, epics and decisions.** Create, edit, delete, numbering, index sync (F3, F4, F11).
3. **M3: AI chat.** Streaming chat against an OpenAI-compatible backend, with document context (F6, F10).
4. **M4: AI edits.** Diff proposals, built-in prompts, `AGENTS.md` help (F5, F7).
5. **M5: Tickets.** Ticket generation and storage (F9).
6. **M6: Polish.** External change detection, size warnings, docs, release builds.

## 12. Open questions

None at the moment.
