#!/usr/bin/env bash
# End-to-end ask, hermetic: the real daemon and friday-ctl, with the fake
# model (fake_model.py) as the only AI provider and no network or desktop.
# It asks the way `friday ask` does (friday-ctl goal --wait) and requires:
#   - every goal finishes (a missing desktop tool once made goals hang),
#   - the daemon reaches the configured model for the goal,
#   - the daemon doesn't panic.
#
# Usage: tests/e2e/ask.sh [bin-dir]   (default target/debug)
set -euo pipefail
cd "$(dirname "$0")/../.."
BIN="${1:-target/debug}"
for b in friday-linux-daemon friday-ctl; do
  [[ -x "$BIN/$b" ]] || { echo "missing $BIN/$b (cargo build -p friday-mobile --bin friday-linux-daemon --bin friday-ctl)" >&2; exit 2; }
done

W=$(mktemp -d)
FAKE="" DAEMON=""
cleanup() { kill $DAEMON $FAKE 2>/dev/null || true; wait 2>/dev/null || true; }
trap cleanup EXIT
fails=0
pass() { echo "  ok   $*"; }
fail() { echo "  FAIL $*"; fails=$((fails + 1)); }

python3 tests/e2e/fake_model.py "$W/port" "$W/model.log" &
FAKE=$!
for _ in $(seq 50); do [[ -s "$W/port" ]] && break; sleep 0.1; done
PORT=$(cat "$W/port")

# No proxies, so no real provider is reachable: the fake model is the only one.
offline=(env -u HTTPS_PROXY -u HTTP_PROXY -u https_proxy -u http_proxy -u ALL_PROXY -u all_proxy
  FRIDAY_STATE_FILE="$W/state.json" FRIDAY_MEMORY_DIR="$W/memory" FRIDAY_LISTEN=127.0.0.1:0
  LLAMACPP_ENABLED=1 LLAMACPP_BASE_URL="http://127.0.0.1:$PORT/v1")
"${offline[@]}" "$BIN/friday-linux-daemon" >"$W/daemon.log" 2>&1 &
DAEMON=$!
for _ in $(seq 120); do grep -q '"status": *"running"' "$W/state.json" 2>/dev/null && break; sleep 0.5; done
if grep -q '"status": *"running"' "$W/state.json" 2>/dev/null; then pass "daemon running"
else fail "daemon never reported running"; tail -20 "$W/daemon.log"; exit 1; fi

echo "ask"
for q in "Summarise this text: The quick brown fox jumps over the lazy dog." \
         "What does the Rust error E0308 mean?"; do
  out=$("${offline[@]}" timeout 90 "$BIN/friday-ctl" goal --wait "$q" 2>&1) || true
  if grep -q '"outcome": *"completed"' <<<"$out"; then pass "completes: $q"
  else fail "did not complete: $q"; sed 's/^/       /' <<<"$out" | tail -8; fi
done

calls=$(grep -c '' "$W/model.log" 2>/dev/null || echo 0)
if (( calls >= 2 )); then pass "daemon reached the configured model ($calls calls)"
else fail "the model was not called (calls: $calls)"; fi
if grep -q "panicked" "$W/daemon.log"; then fail "daemon panicked:"; grep -A1 panicked "$W/daemon.log" | head -4
else pass "no panics"; fi

if (( fails > 0 )); then echo "e2e ask: $fails failure(s) (logs in $W)"; exit 1; fi
echo "e2e ask: ok"
