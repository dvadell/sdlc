---
status: done
depends_on: []
---

# 0001: Project skeleton and required tooling

**Epic:** [0001 Foundation and server shell](../description.md) · **Depends on:** none · **Milestone:** M1

## Goal

Turn this docs-only repository into a Rust project that builds, tests and lints
with the toolchain the conventions require. Every later ticket assumes this.

## Build

- `Cargo.toml`: package `sdlc`, edition 2024, pinned `rust-version`, binary
  target plus a `src/lib.rs` so integration tests can import the crate.
- `src/lib.rs` with `#![forbid(unsafe_code)]` and a placeholder `run()` that
  returns `Ok(())`; `src/main.rs` calls it and prints nothing yet.
- `rust-toolchain.toml` with `channel = "stable"` and components `rustfmt`,
  `clippy`.
- `justfile` with `run`, `fmt`, `lint`, `test`, `check`, `release`. `check` runs
  `cargo fmt --all -- --check`, `cargo clippy --all-targets -- -D warnings` and
  `cargo test`.
- `.gitignore` with `/target`; commit `Cargo.lock` (binary crate).

## Already in place

- `sdlc/PRD.md`, `sdlc/architecture.md`, `sdlc/conventions.md`.
- No source tree, no tooling.

## Acceptance criteria

- [ ] `cargo build`, `cargo test` and the clippy command above pass.
- [ ] `just check` runs all three checks and fails when one fails.
- [ ] Edition 2024 and `rust-version` agree with the installed stable toolchain.
- [ ] No dependency is added yet; 0002 introduces `clap`.

## Tests

- **Build:** `cargo test` compiles the lib and the empty test suite.
- **Tooling:** deliberately break formatting once and confirm `just check`
  fails; revert.
- **Manual:** `just --list` shows the documented tasks.

## Gotchas

- Edition 2024 needs Rust 1.85 or newer. If the machine is older, stop and
  record the constraint instead of silently downgrading the edition.
- Keep `lib.rs` and `main.rs` separate from day one; converting a binary-only
  crate later breaks every integration test.
- Do not create `frontend/` yet; 0005 owns it.
