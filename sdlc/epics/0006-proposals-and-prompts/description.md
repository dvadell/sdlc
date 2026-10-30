---
status: planned
---

# 0006: Proposals and prompts

*Tickets 0040–0047 · Milestone M4*

## Outcome

The model can propose full-file changes using plain-text blocks. The server
validates every path, computes a unified diff and holds the proposal in memory.
The person reviews one diff and accepts or rejects the whole proposal. Built-in
prompts cover the common documentation jobs, including trimming `AGENTS.md`.

## Why this order

The block parser and diff builder are pure and come first. Accepting has the
concurrency rules, so it comes before the HTTP surface. Chat wiring comes after
the service exists, and the UI comes last. The AI still cannot write to disk;
only an accepted proposal writes.

## Tickets

| # | Title | Depends on |
|---|-------|------------|
| [0040](tickets/0040-block-parser.md) | FILE and DELETE block parser | 0008 |
| [0041](tickets/0041-proposal-build.md) | Build proposals and diffs | 0010, 0040 |
| [0042](tickets/0042-accept-reject.md) | Accept and reject proposals | 0011, 0029, 0041 |
| [0043](tickets/0043-proposal-routes.md) | Proposal API routes | 0007, 0042 |
| [0044](tickets/0044-chat-proposals.md) | Wire chat replies into proposals | 0036, 0041 |
| [0045](tickets/0045-diff-ui.md) | Diff view UI | 0023, 0043, 0044 |
| [0046](tickets/0046-built-in-prompts.md) | Built-in prompt shortcuts | 0038 |
| [0047](tickets/0047-agents-help.md) | AGENTS.md editing help | 0038, 0046 |

## Exit criteria

- A model reply with `FILE` and `DELETE` blocks becomes a proposal with a diff.
- Any invalid path rejects the whole proposal, with a reason in the chat.
- Accept re-checks hashes; a stale file means nothing is written.
- Reject discards without touching disk.
- The diff view shows additions and deletions and has only whole-proposal
  actions.
- Malformed blocks are ignored with a visible warning.

## Risks

- Models wrap output in code fences or invent paths; 0040 and 0044 must be
  forgiving about blocks and strict about paths.
- Multi-file accept is not a transaction. 0042 reports exactly what was
  written if a later write fails.
- Proposal acceptance races with external edits; hashes are the guard, not
  locks.
