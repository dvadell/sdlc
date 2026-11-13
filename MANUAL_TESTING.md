# Manual testing checklist

Release gate before calling v1 done (ticket 0061).

- Date: 2026-11-13
- Commit: `8b11b41` (`0060: verify the release binary is self-contained review fixes`)
- Platform: Linux (x86_64)
- Browser: not browser-verified in this run (manual)
- AI server model: local OpenAI-compatible (`qwen3.8:27b-mlx` via `http://host.containers.internal:11434/v1`)

Run against a fresh project, with and without an AI backend. File each failure
as a new ticket instead of fixing it silently in this one; re-run the affected
section after a fix.

## Fresh project

- [ ] An empty directory scaffolds through onboarding and the core documents
      open.

## Editing

- [ ] An epic, feature, decision and ticket can be created, renamed and deleted.
- [ ] Core documents are protected against delete and overwrite.
- [ ] A non-empty epic delete is refused.
- [ ] The unsaved indicator appears and clears with the save state.
- [ ] Ctrl/Cmd+S saves.
- [ ] CRLF preservation of document content.

## Conflicts

- [ ] An external edit followed by a save resolves through reload or overwrite.
- [ ] A deleted document is reported on save or reload.

## Index

- [ ] A feature status, title, rename or delete updates the PRD index region only.

## AI unconfigured

- [ ] Editing works without an AI backend.
- [ ] The setup instructions name the `SDLC_AI_*` environment variables.

## AI configured (local)

- [ ] The stream delivers file-change and chat events.
- [ ] The stream can be stopped and errors surface.
- [ ] No chat history survives a reload.

## Proposals

- [ ] An accept, a reject, a conflicting accept and a partial failure.
- [ ] A malformed block warns.
- [ ] An invalid path is rejected.

## AGENTS.md

- [ ] The prompt trim, the diff and the accept of an `AGENTS.md` proposal.
- [ ] A `CLAUDE.md` proposal is rejected.

## Tickets

- [ ] A ticket is generated into a chosen epic, reviewed, accepted, hand-edited,
      regenerated, and the malformed-epic unassigned group is exercised.

## Watcher

- [ ] An external edit reloads a clean open document, warns on a dirty one, and
      the app's own saves stay silent.

## Warnings

- [ ] The over-limit PRD warning, the code-fence warning and the stack-heading
      warning render.

## Security

- [ ] An XSS in a document and in a model reply.
- [ ] A bad `Host`/`Origin` via curl is refused.
- [ ] Traversal attempts are refused.

## Release

- [ ] The 0060 release check script and the release binary outside the repository
      pass.

## Run record

- Full run against a clean temp project: pending (this checklist is the gate
      artifact; the run itself is performed against a live server with a browser).
- Both AI-configured and AI-unconfigured runs: recorded per run.
- Failures: none outstanding at write time; any observed failure becomes a new
      ticket.

## Success criteria

- [ ] The fresh-project path was checked end to end.
- [ ] Every feature section above was exercised or explicitly waived.
- [ ] The PRD success criteria each carry evidence.
