#!/usr/bin/env bash
# End-to-end ask, hermetic: the real daemon and friday-ctl, with the fake
# model (fake_model.py) as the only AI provider and no network or desktop.
# It asks the way `friday ask` does (friday-ctl goal --wait) and requires:
#   - every goal finishes (a missing desktop tool once made goals hang),
#   - the answer friday-ctl prints is the model's reply to that question
#     (the answer step, and its result reaching state.json),
#   - the daemon doesn't panic.
#
# Usage: tests/e2e/ask.sh [bin-dir]   (default target/debug)
# Works on Linux and in Git Bash on Windows, where bin-dir can be the MSI's
# install folder (the daemon is friday-daemon.exe there).
set -euo pipefail
cd "$(dirname "$0")/../.."
BIN="${1:-target/debug}"
pick() { local f; for f in "$@"; do [[ -x "$BIN/$f" ]] && { echo "$BIN/$f"; return; }; done; }
DAEMON_BIN=$(pick friday-linux-daemon friday-linux-daemon.exe friday-daemon.exe)
CTL_BIN=$(pick friday-ctl friday-ctl.exe)
if [[ -z "$DAEMON_BIN" || -z "$CTL_BIN" ]]; then
  echo "no daemon/friday-ctl in $BIN (cargo build -p friday-mobile --bin friday-linux-daemon --bin friday-ctl)" >&2
  exit 2
fi
PY=$(command -v python3 || command -v python)

W=$(mktemp -d)
FAKE="" DAEMON=""
cleanup() { kill $DAEMON $FAKE 2>/dev/null || true; wait 2>/dev/null || true; }
trap cleanup EXIT
fails=0
pass() { echo "  ok   $*"; }
fail() { echo "  FAIL $*"; fails=$((fails + 1)); }

"$PY" tests/e2e/fake_model.py "$W/port" "$W/model.log" &
FAKE=$!
for _ in $(seq 50); do [[ -s "$W/port" ]] && break; sleep 0.1; done
PORT=$(cat "$W/port")

# No proxies, so no real provider is reachable: the fake model is the only one.
offline=(env -u HTTPS_PROXY -u HTTP_PROXY -u https_proxy -u http_proxy -u ALL_PROXY -u all_proxy
  FRIDAY_STATE_FILE="$W/state.json" FRIDAY_MEMORY_DIR="$W/memory" FRIDAY_LISTEN=127.0.0.1:0
  LLAMACPP_ENABLED=1 LLAMACPP_BASE_URL="http://127.0.0.1:$PORT/v1")
"${offline[@]}" "$DAEMON_BIN" >"$W/daemon.log" 2>&1 &
DAEMON=$!
for _ in $(seq 120); do grep -q '"status": *"running"' "$W/state.json" 2>/dev/null && break; sleep 0.5; done
if grep -q '"status": *"running"' "$W/state.json" 2>/dev/null; then pass "daemon running"
else fail "daemon never reported running"; tail -20 "$W/daemon.log"; exit 1; fi

echo "ask"
for q in "Summarise this text: The quick brown fox jumps over the lazy dog." \
         "What does the Rust error E0308 mean?"; do
  out=$("${offline[@]}" timeout 90 "$CTL_BIN" goal --wait "$q" 2>&1) || true
  if grep -q '"outcome": *"completed"' <<<"$out"; then pass "completes: $q"
  else fail "did not complete: $q"; sed 's/^/       /' <<<"$out" | tail -8; continue; fi
  answer=$("$PY" -c 'import json,sys
objs=[]; d=json.JSONDecoder(); s=sys.stdin.read(); i=0
while (j := s.find("{", i)) >= 0:
    try: o, n = d.raw_decode(s[j:]); objs.append(o); i = j + n
    except ValueError: i = j + 1
done=[o for o in objs if isinstance(o, dict) and "outcome" in o]
print((done[-1].get("result") or "") if done else "")' <<<"$out")
  if [[ "$answer" == "FAKE-MODEL-ANSWER: $q"* ]]; then pass "answered by the model: ${answer:0:70}"
  else fail "answer is not the model's reply to the question (got: '${answer:0:120}')"; fi
done

answers=$(grep -vc "Investigation Engine" "$W/model.log" 2>/dev/null || echo 0)
if (( answers >= 2 )); then pass "one answer call per question ($answers)"
else fail "expected an answer call per question (got $answers)"; fi
if grep -q "panicked" "$W/daemon.log"; then fail "daemon panicked:"; grep -A1 panicked "$W/daemon.log" | head -4
else pass "no panics"; fi

if (( fails > 0 )); then echo "e2e ask: $fails failure(s) (logs in $W)"; exit 1; fi
echo "e2e ask: ok"
