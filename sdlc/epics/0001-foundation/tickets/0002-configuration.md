---
status: todo
depends_on: ["0001"]
---

# 0002: CLI and environment configuration

**Epic:** [0001 Foundation and server shell](../description.md) · **Depends on:** 0001 · **Milestone:** M1

## Goal

Parse flags and environment variables once at startup into an immutable
`Config`. The rest of the program never reads `std::env` or CLI arguments.

## Build

- `src/config.rs` with:
  - `pub struct Config { project_dir: PathBuf, port: u16, open: bool, ai: Option<AiConfig> }`
  - `pub struct AiConfig { base_url: String, model: String, api_key: Option<ApiKey> }`
  - `ApiKey` newtype with a redacting `Debug`; no `Serialize`.
- Flags via `clap` derive: `--dir` (default `.`), `--port` (default `9000`),
  `--open`.
- Env vars: `SDLC_AI_BASE_URL`, `SDLC_AI_MODEL`, `SDLC_AI_API_KEY`.
- A pure `Config::from_parts(args, lookup_env)` constructor so tests do not
  mutate process-global environment state.
- Validation: `--dir` must exist and be a directory; canonicalize it. Port is
  any `u16` including `0` (ephemeral, used by tests). Trim trailing `/` from the
  base URL.
- AI is configured only when both base URL and model are non-empty; the key is
  optional. Log a short warning naming missing variables when disabled.

## Already in place

- Crate skeleton and `just check` from 0001.

## Acceptance criteria

- [ ] Defaults are `.`, `9000`, `--open` off.
- [ ] Flags override defaults; env vars populate `AiConfig`.
- [ ] Missing base URL or model yields `ai: None`, never a half-built config.
- [ ] A key without URL/model is ignored with a warning, not fatal.
- [ ] Invalid `--dir` exits with a readable message, not a panic.
- [ ] Debug output and errors never contain the key.

## Tests

- **Unit:** defaults; each flag; both AI vars; only one AI var; key optional;
  trailing slash trimmed; redacted `Debug`; directory missing.
- **Unit:** `SDLC_AI_API_KEY` alone produces `ai: None`.
- **Manual:** `cargo run -- --help` shows the three flags and defaults.

## Gotchas

- Tests run in parallel; never call `std::env::set_var`. Pass an env lookup
  closure instead.
- Canonicalizing `--dir` makes later path checks simpler, but it also resolves
  symlinks: document that the project root is the canonical directory.
- Do not derive `Debug` on `ApiConfig`/`Config` until the key is redacted;
  `tracing::info!(?config)` is an easy accidental leak.

## Notes

- Crate context: `sdlc::run() -> Result<(), std::io::Error>` is in `src/lib.rs`
  and called from `src/main.rs`; server startup (0004) and `AppState` (0007) consume
  `Config`, so keep the field set stable for those tickets.
- No dependencies exist yet; adding `clap = { version = "4", features = ["derive"] }`
  is expected in this ticket.
- The pure `Config::from_parts` may take parsed args plus an env lookup closure;
  it must not touch process-global state or argv.
- The invalid-`--dir` error type is not fixed here; conventions call for a module
  error enum via `thiserror` (no `anyhow`/`Box<dyn Error>` in library code).
- Naming for the AI config and its key newtype is not fixed here; pick one and stay
  consistent with `conventions.md` and `architecture.md`.
