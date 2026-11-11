 Per-ticket cycle (for a new worker / AI):

 1. Pick ticket — next unchecked in sdlc/epics/*/tickets/, in order, deps green.
 2. Delegate — task → ticket-solver or build with self-contained task (# Goal / # Contract / # Target / # Change / # Acceptance). If you need to add something for context, add it in the ticket itself, under Notes. The delegation call to the agent should be something small like "work on ticket sdlc/epics/*/tickets/some_ticket.md.
 3. Wait for completion; check git status; commit agent work as NNNN: <title>.
 4. Review — ocr review --concurrency 1 --timeout 0 --audience agent --commit <sha>.
 5. Evaluate findings — real bug → fix (+ regression test if practical) → commit fix → re-run review on fix commit → clean. Non-bug → ignore.
 6. Checks — cargo fmt --all, cargo clippy --all-targets -- -D warnings, cargo test --all-targets (plus ticket filter, e.g. presets).
 7. Commit, but don't push. One commit per ticket. If there are changes after the review, use `git commit --amend` and review again.
 8. Save learned lessons in the "Gotchas learned" session from this document, if any. A learned lesson must be useful for future development.

 Gotchas learned:
 - Agent may fail to land edits → retry agent or implement directly; verify via git status.
 - ocr review can fail transiently (task_done FAILED / LLM error / JSON parse / timeout) → re-run same command; --resume <sid> --from/--to/--commit if needed.
 - Review may hit tool-request caps / LLM parse errors but still emit findings/completion.
 - ocr review skips sdlc/tickets/**.
- When ocr review fails every request with "network error", the configured LLM endpoint may be down: check with `ocr llm test` and simply re-run the same review once it answers; do not switch providers or models.
- Review findings can be wrong: check them against the relevant spec before editing (e.g. a bracketed IPv6 URI authority `[::1]:11434` is already correct, so stripping the brackets would make it invalid).
 - Delegation to the ticket-solver or build agent may take up to ~45 min; use a longer wait timeout when polling for completion.
 - ocr review background jobs can be killed by the harness around ~20 min even with `--timeout 0`; re-run the same command.
 - The task delegation tool rejected calls whose tasks member was sent as multi-line JSON, replying that a tasks array was missing; sending tasks as a single-line JSON array string works.

 - SubscriberInitExt::set_default (scoped/thread-local) also calls tracing_log::LogTracer::init(), which sets the global log dispatcher; a later init_tracing() try_init then fails with SetLoggerError. Use tracing::subscriber::with_default (scoped) in tests instead of set_default.

- axum 0.8 has no `with_ctrl_c`; use `Serve::with_graceful_shutdown` with an async future that awaits `tokio::signal::ctrl_c()` for Ctrl-C shutdown.
- tokio features used directly by sdlc (`net`, `signal`, `io-util`, `macros`/`rt`, `time`) must be declared in `Cargo.toml` even when axum pulls some in transitively; without them the server types and async test helpers used here do not compile.

- A delegated agent can hit the provider free-usage limit mid-task (the ticket-solver
  session died this way), leaving partial edits behind. After every delegation, check
  `git status` / `git diff` before committing; untracked files under `tests/` may be
  half-finished solver output and must be reviewed before being committed.

- `ocr review` can fail transiently with network errors partway through a review
  (observed on ticket 0015: the run failed and the retry with `--resume` completed
  cleanly). When a review fails, re-run the same command with `--resume <session-id>`
  before assuming the code has a problem.
- `ocr review` on a fix commit can come back with findings that target the fix
  itself (e.g. offsets like `marker_end + 1`, byte-slice terminator handling); expect
  iterative review rounds and re-run the review on the amended fix commit until it
  comes back clean (0 findings).
- The decision-create dialog settles its promise from inside a native `<dialog>`
  setup that runs synchronously, so a test that makes `showModal` fail must install
  the failure before the dialog is created (patch the prototype), not on the dialog
  instance afterwards — an instance override installed afterwards never runs.
- A `stream::unfold` closure handed to the stream machinery is `FnMut`, so it may
  not move a captured variable into the `async move` block it returns; clone the
  captured value at the closure-body scope (before the `async move`) and consume the
  local clone inside, or the borrow checker rejects the move with E0507.

Important: Delegate the task to the ticket-solver or build agent. Do not do it yourself.
