---
status: todo
depends_on: ["0035", "0044", "0048"]
---

# 0049: Ticket generation prompt and context

**Epic:** [0007 Ticket generation](../description.md) · **Depends on:** 0035, 0044, 0048 · **Milestone:** M5

## Goal

Turn a feature spec into proposed ticket files grouped under one epic, with the
feature, epic, conventions and a clear per-ticket structure in context.

## Build

- Add the generation prompt to `prompts.js` (replacing the disabled entry from
  0046): requires an open feature or a chosen feature and a chosen epic; if
  either is missing, explain what to select.
- Attach the feature spec, the epic document and `sdlc/conventions.md`; avoid
  sending the open document twice (0039 dedupes).
- User message must ask for:
  - one `FILE` block per ticket under
    `sdlc/epics/<epic>/tickets/<kebab-slug>.md`, **without a numeric prefix**
    (0050 assigns it);
  - front matter optional `feature: <slug>`, `status: todo`, `depends_on: []`;
    do **not** write an `epic` key, because the path determines the epic;
  - H1 `# <Title>` without a number (0050 adds it);
  - sections Description, Acceptance criteria, Tests, Gotchas, Dependencies;
  - dependencies written as ticket titles in the body, because numbers do not
    exist yet;
  - small tickets, one sitting each.
- Keep the text in `prompts.js`; no server-side prompt endpoint.
- The chat request needs to tell the proposal layer that these creates are
  ticket drafts so 0050 can normalize them. Add an optional
  `ticket_context: { feature: slug, epic: "0001-foundation" }` field to
  `POST /api/chat`, passed to the prompt builder and to proposal creation. The
  epic value is the exact directory name.

## Already in place

- Prompt builder, proposal flow, ticket metadata.

## Acceptance criteria

- [ ] The prompt appears only when a feature and an epic are selected.
- [ ] The request contains the feature, epic and conventions.
- [ ] The model is asked for one file per ticket, no numbers, under the selected
      epic's `tickets/` directory.
- [ ] `ticket_context.feature` and `ticket_context.epic` reach the proposal
      builder.
- [ ] A reply with two ticket blocks yields two proposed creates in that epic.
- [ ] Invalid or non-ticket paths still reject the whole proposal.
- [ ] The requested front matter has no `epic` key.

## Tests

- **Unit:** assembled prompt contains the feature slug, epic directory,
  conventions and the no-number rule.
- **Integration (wiremock):** body includes `ticket_context`; reply blocks
  produce a proposal (numbering in 0050).
- **Manual:** run against a local model on a real feature and epic; inspect the
  reply and diff before accepting.
- **Manual:** invoke with no feature or no epic selected; the button explains
  what to choose.

## Gotchas

- Asking a model for numbers produces collisions; the prompt must forbid them
  even though 0050 can normalize them.
- Dependencies cannot be final numbers before allocation. Do not promise
  machine-readable links that the flow cannot produce; body text is the v1
  answer.
- The feature and epic documents plus conventions can be large; keep other
  attachments off by default.
- Do not silently create tickets without the diff review.
