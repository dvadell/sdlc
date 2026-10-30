---
status: todo
depends_on: ["0038"]
---

# 0046: Built-in prompt shortcuts

**Epic:** [0006 Proposals and prompts](../description.md) · **Depends on:** 0038 · **Milestone:** M4

## Goal

One-click prompts for the six documentation jobs in the PRD, with the right
documents attached automatically.

## Build

- `frontend/js/prompts.js`: an array of `{ id, label, text, attachments }`.
- Buttons in the chat panel, e.g. a small "Prompts" row or select. Clicking
  fills the input with the prompt text and adds the attachments; it does not
  send immediately, so the person can adjust.
- Prompts:
  1. **Split this PRD by concern** — attach PRD; ask for the split into
     architecture/conventions/features with `FILE` blocks.
  2. **Draft a feature spec from this idea** — attach PRD.
  3. **Draft an ADR for this choice** — attach architecture + conventions.
  4. **Trim AGENTS.md** — attach AGENTS + conventions (detailed in 0047).
  5. **Check docs for contradictions or duplication** — attach PRD,
     architecture, conventions, AGENTS.
  6. **Generate tickets from this feature** — placeholder until 0049; either
     hidden or disabled with a tooltip until that ticket lands.
- Keep all prompt text in this one module; no server-side prompt catalog.
- Add `js/prompts.js` to the file-responsibility table in `conventions.md`
  §11.
- Attachments follow 0039 and are added, not replaced, unless the prompt says
  "replace".
- Show a short description as the button's `title`.

## Already in place

- Chat panel, attachments, proposal flow.

## Acceptance criteria

- [ ] Each button fills the input and attaches the intended docs.
- [ ] Nothing is sent until the person presses Enter/Send.
- [ ] Duplicate attachments are not created.
- [ ] Prompt text mentions the expected output and can be edited.
- [ ] The ticket prompt is disabled until 0049 exists.
- [ ] Long prompt text does not overflow the panel.

## Tests

- **Manual:** click each prompt, inspect chips and input, then send one against
  a fake/local model.
- **Manual:** click a prompt twice; attachments do not duplicate.
- **Manual:** remove an auto-attached doc before sending; the request reflects
  it.
- **Manual:** the "split PRD" reply produces a proposal (manual end-to-end).

## Gotchas

- Auto-attaching four docs can exceed a small local model's context; that is a
  documented v1 limitation, but keep the attachment list visible so the person
  can trim it.
- Do not hard-code prose that contradicts the system prompt's proposal format.
- A prompt that expects a proposal should say "propose full files", not "show
  a patch".
- Keep labels short; the panel is narrow.
