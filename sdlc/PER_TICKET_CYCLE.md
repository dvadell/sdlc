 Per-ticket cycle (for a new worker / AI):

 1. Pick ticket — next unchecked in sdlc/epics/*/tickets/, in order, deps green.
 2. Delegate — task → ticket-solver with self-contained task (# Goal / # Contract / # Target / # Change / # Acceptance). If you need to add something for context, add it in the ticket itself, under Notes. The delegation call to the agent should be something small like "work on ticket sdlc/epics/*/tickets/some_ticket.md.
 3. Wait for completion; check git status; commit agent work as NNNN: <title>.
 4. Review — ocr review --concurrency 1 --timeout 0 --audience agent --commit <sha>.
 5. Evaluate findings — real bug → fix (+ regression test if practical) → commit fix → re-run review on fix commit → clean. Non-bug → ignore.
 6. Checks — cargo fmt --all, cargo clippy --all-targets -- -D warnings, cargo test --all-targets (plus ticket filter, e.g. presets).
 7. Commit, but don't push. One commit per ticket. If there are changes after the review, use `git commit --amend` and review again.
 8. Save learned lessons in the "Gotchas learned" session from this document.

 Gotchas learned:
 - Agent may fail to land edits → retry agent or implement directly; verify via git status.
 - ocr review can fail transiently (task_done FAILED / LLM error / JSON parse / timeout) → re-run same command; --resume <sid> --from/--to/--commit if needed.
 - Review may hit tool-request caps / LLM parse errors but still emit findings/completion.
 - ocr review skips sdlc/tickets/**.
- Delegation to the ticket-solver agent may take up to ~45 min; use a longer wait timeout when polling for completion.
- ocr review background jobs can be killed by the harness around ~20 min even with `--timeout 0`; re-run the same command.

- SubscriberInitExt::set_default (scoped/thread-local) also calls tracing_log::LogTracer::init(), which sets the global log dispatcher; a later init_tracing() try_init then fails with SetLoggerError. Use tracing::subscriber::with_default (scoped) in tests instead of set_default.

Important: Delegate the task to the ticket-solver agent. Do not do it yourself.
