---
status: done
depends_on: ["0021", "0038"]
---

# 0058: README and privacy notice

**Epic:** [0008 Polish and release](../description.md) · **Depends on:** 0021, 0038 · **Milestone:** M6

## Goal

Make the first run understandable in under two minutes and make the data flow
to the AI service impossible to miss.

## Build

- Root `README.md`:
  - what sdlc is, the document layout (including `sdlc/epics/<epic>/`
    `description.md` and `tickets/`), and that files stay plain Markdown;
  - install/build (`cargo build --release`, binary name `sdlc`);
  - run: `sdlc --dir <path> --port 9000 --open`, defaults, loopback only;
  - configuration table for the three `SDLC_AI_*` variables with a local
    OpenAI-compatible example;
  - privacy section: document content is sent to the configured host; the key
    stays in the environment; no chat history is stored;
  - security: no auth, local only, Host/Origin guard, path whitelist;
  - limitations matching the PRD non-goals;
  - development: `just check`, no Node/npm, vendored frontend libs.
- UI: the chat panel already shows the AI host (0038); add a first-run notice
  in the panel or footer that says the same in one sentence, plus a link to the
  README section.
- Do not include real keys, tokens or a hosted service URL in examples.
- Keep the README short; link to the docs for detail.

## Already in place

- Config endpoint, editor, chat panel, guard, store.

## Acceptance criteria

- [ ] A new user can build, run and scaffold from the README alone.
- [ ] Every flag and env var in the code is documented with its default.
- [ ] The privacy statement names the configured host, not a vague "AI".
- [ ] No claim exceeds v1 (no network hosting, no history, no source access).
- [ ] The UI notice appears when AI is configured and when it is not.
- [ ] Links and code blocks render correctly on GitHub.

## Tests

- **Manual:** follow the README from a clean checkout in a temp directory.
- **Manual:** compare the documented env vars with `--help` and
  `/api/config`.
- **Manual:** read the privacy text with a configured host and without.
- **Manual:** search the repo for real-looking keys before publishing.

## Gotchas

- A README that promises context management or git commits contradicts the
  PRD.
- The API key is optional for local servers; saying "required" costs a support
  round trip.
- The UI notice must not print the full base URL if it could contain userinfo;
  use `ai_host`.
- Keep the README in sync when 0057 adds `SDLC_PRD_MAX_BYTES`.
