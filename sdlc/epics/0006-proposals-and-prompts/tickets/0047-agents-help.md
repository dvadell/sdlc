---
status: todo
depends_on: ["0038", "0046"]
---

# 0047: AGENTS.md editing help

**Epic:** [0006 Proposals and prompts](../description.md) · **Depends on:** 0038, 0046 · **Milestone:** M4

## Goal

Make `AGENTS.md` a first-class editable document and give the person a reliable
way to keep it short and consistent with the other docs.

## Build

- Confirm `AGENTS.md` appears in the Core group, opens in the editor and saves
  like any other document. Delete must remain unavailable.
- Add a dedicated prompt (wired in 0046): "Trim `AGENTS.md` and keep it
  consistent with the PRD, architecture and conventions. Keep rules actionable
  and short; propose the full new file." Attach `AGENTS.md`, PRD, architecture
  and conventions.
- System context must state that `AGENTS.md` is the only rules file and that
  `CLAUDE.md` is not managed. A proposal touching `CLAUDE.md` is rejected by
  `DocPath`; the error should be readable.
- Add a short note in the AGENTS editor area: AI proposals replace the whole
  file; review the diff.
- Do not automatically rewrite `AGENTS.md` on other operations.

## Already in place

- Core editing, attachments, prompts, proposal diff.

## Acceptance criteria

- [ ] `AGENTS.md` lists, opens, edits and saves.
- [ ] No delete action is shown or accepted by the API.
- [ ] The trim prompt attaches the four documents.
- [ ] A proposal for `AGENTS.md` shows a diff and waits for acceptance.
- [ ] A model reply proposing `CLAUDE.md` fails with a clear invalid-path
      message and writes nothing.
- [ ] Refreshing the page shows the accepted content from disk.

## Tests

- **Manual:** run the trim prompt against a local model, review, accept, diff
  the file.
- **Manual:** reject; the file is unchanged.
- **Integration:** a proposal containing `CLAUDE.md` is rejected as a whole.
- **Manual:** create a large `AGENTS.md` (over the model context) and confirm
  the failure is reported, not truncated silently.

## Gotchas

- `AGENTS.md` may contain instructions aimed at an AI. Sending it as context is
  prompt injection by design; only the proposal diff is actionable, so treat
  its content as untrusted text.
- Do not add `CLAUDE.md` writing "for compatibility"; the PRD explicitly
  excludes it.
- The trim prompt can produce a large diff; the whole-proposal rule means one
  bad line rejects everything. Say so in the panel hint.
- Keep the privacy notice visible in the chat panel; this flow sends the most
  sensitive document set.
