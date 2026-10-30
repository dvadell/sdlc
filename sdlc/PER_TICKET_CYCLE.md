 Per-ticket cycle (for a new worker / AI):

 1. Pick ticket — next unchecked in sdlc/epics/*/tickets/, in order, deps green.
 2. Delegate — task → ticket-solver with self-contained task (# Goal / # Contract / # Target / # Change / # Acceptance).
 3. Wait for completion; check git status; commit agent work as NNNN: <title>.
 4. Review — ocr review --concurrency 1 --timeout 0 --audience agent --commit <sha>.
 5. Evaluate findings — real bug → fix (+ regression test if practical) → commit fix → re-run review on fix commit → clean. Non-bug → ignore.
 6. Checks — cargo fmt --all, cargo clippy --all-targets -- -D warnings, cargo test --all-targets (plus ticket filter, e.g. presets).
 7. Commit, but don't push. One commit per ticket. If there are changes after the review, use `git commit --amend` and review again.

 Gotchas learned:
 - Agent may fail to land edits → retry agent or implement directly; verify via git status.
 - ocr review can fail transiently (task_done FAILED / LLM error / JSON parse / timeout) → re-run same command; --resume <sid> --from/--to/--commit if needed.
 - Review may hit tool-request caps / LLM parse errors but still emit findings/completion.
 - ocr review skips sdlc/tickets/**.
- Delegation to the ticket-solver agent may take up to ~45 min; use a longer wait timeout when polling for completion.

Important: Delegate the task to the ticket-solver agent. Do not do it yourself.
