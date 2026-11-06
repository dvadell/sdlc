# sdlc task runner.

# Run the binary.
run:
    cargo run

# Format the Rust sources.
fmt:
    cargo fmt --all

# Lint the Rust sources.
lint:
    cargo clippy --all-targets -- -D warnings

# Run the test suite.
test:
    cargo test

# Run the frontend API and editor tests.
frontend-test:
    node --test 'frontend/js/*.test.mjs'

# Run all checks: format, lint, and tests.
check:
    cargo fmt --all -- --check
    cargo clippy --all-targets -- -D warnings
    cargo test
    just frontend-test

# Build the release binary.
release:
    cargo build --release