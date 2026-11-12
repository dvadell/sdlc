#!/usr/bin/env bash
# Proves the release binary is self-contained (ticket 0060).
#
# 1. Builds the release binary.
# 2. Copies it to a fresh temp dir with no repository.
# 3. Starts it with `--dir <empty temp project>` on port `0`.
# 4. Requests `/` and `/api/config`.
# 5. Stops it and fails on non-200 or missing markers.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BINARY="$REPO_ROOT/target/release/sdlc"
WORKDIR="$(mktemp -d)"
PROJECT="$WORKDIR/project"
PID=""

cleanup() {
    if [ -n "$PID" ]; then
        kill "$PID" 2>/dev/null || true
        wait "$PID" 2>/dev/null || true
    fi
    rm -rf "$WORKDIR"
}
trap cleanup EXIT

fail() {
    echo "check-release: $*" >&2
    exit 1
}

[ -x "$BINARY" ] || fail "the release binary is missing; run 'just release' first"

cargo build --release

cp "$BINARY" "$WORKDIR/sdlc"
chmod +x "$WORKDIR/sdlc"
mkdir "$PROJECT"

VERSION_OUTPUT="$("$WORKDIR/sdlc" --version)"
EXPECTED_VERSION="${VERSION_OUTPUT##* }"

cd "$WORKDIR"
"$WORKDIR/sdlc" --dir "$PROJECT" --port 0 > "$WORKDIR/server.log" 2>&1 &
PID=$!

for _ in $(seq 1 60); do
    if grep -q "listening on" "$WORKDIR/server.log" 2>/dev/null; then
        break
    fi
    if ! kill -0 "$PID" 2>/dev/null; then
        cat "$WORKDIR/server.log" >&2
        fail "the release binary exited before listening"
    fi
    sleep 1
done

grep -q "listening on" "$WORKDIR/server.log" || fail "the release binary did not start listening"

LISTENING="$(grep -m1 'listening on' "$WORKDIR/server.log")"
PORT="${LISTENING##*:}"
case "$PORT" in
    ''|*[!0-9]*) fail "could not parse the listening port from: $LISTENING" ;;
esac

CODE=""
for _ in $(seq 1 30); do
    CODE="$(curl -s -o /dev/null -w '%{http_code}' -H "Host: 127.0.0.1:$PORT" "http://127.0.0.1:$PORT/api/config" || true)"
    if [ "$CODE" = "200" ]; then
        break
    fi
    if ! kill -0 "$PID" 2>/dev/null; then
        cat "$WORKDIR/server.log" >&2
        fail "the release binary exited before serving /api/config"
    fi
    sleep 1
done

[ "$CODE" = "200" ] || fail "GET /api/config answered with status $CODE"

PAGE_CODE="$(curl -s -o "$WORKDIR/page.html" -w '%{http_code}' -H "Host: 127.0.0.1:$PORT" "http://127.0.0.1:$PORT/")"
[ "$PAGE_CODE" = "200" ] || fail "GET / answered with status $PAGE_CODE"

grep -q 'id="sidebar"' "$WORKDIR/page.html" || fail "GET / did not answer with the entry page marker"

PAGE_CONTENT_TYPE="$(curl -s -o /dev/null -w '%{content_type}' -H "Host: 127.0.0.1:$PORT" "http://127.0.0.1:$PORT/")"
case "$PAGE_CONTENT_TYPE" in
    text/html*) ;;
    *) fail "GET / answered with content type $PAGE_CONTENT_TYPE" ;;
esac

CONFIG_CODE="$(curl -s -o "$WORKDIR/config.json" -w '%{http_code}' -H "Host: 127.0.0.1:$PORT" "http://127.0.0.1:$PORT/api/config")"
[ "$CONFIG_CODE" = "200" ] || fail "GET /api/config answered with status $CONFIG_CODE"

jq -e . "$WORKDIR/config.json" > /dev/null 2>&1 || fail "GET /api/config did not answer with JSON"

jq -e --arg version "$EXPECTED_VERSION" \
    'type == "object" and .version == $version and .ai_configured == false and (.port | type == "number")' \
    "$WORKDIR/config.json" > /dev/null || fail "GET /api/config answered with an unexpected payload"

kill "$PID" 2>/dev/null || true
wait "$PID" 2>/dev/null || true
PID=""

echo "check-release: the release binary served the embedded frontend from an empty directory"
