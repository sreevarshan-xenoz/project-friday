#!/usr/bin/env bash
# End-to-end desktop control on Windows, hermetic: the real daemon and
# friday-ctl, the real FRIDAY Windows host (the C# service that lists and
# acts on windows), and the fake model scripted to list the windows and
# finish. It requires:
#   - the goal "focus a window" is routed to desktop control and completes,
#   - the model's `windows` action ran against the host, and was recorded in
#     actions.jsonl as a successful read (the audit trail),
#   - the model was given the Windows instructions (not FRIDAY OS's).
#
# It also asks for the volume, the music and what is playing. A runner has no
# speakers and nothing playing, so the host must answer each with its own plain
# sentence (or succeed, on a machine that has them), never an exception.
#
# Usage: tests/e2e/desktop.sh <install-dir>   (the MSI's folder; Git Bash on
# Windows, in a desktop session)
set -euo pipefail
cd "$(dirname "$0")/../.."
BIN="${1:?usage: desktop.sh <folder with friday-daemon.exe, friday-ctl.exe, friday-windows-host.exe>}"
DAEMON_BIN="$BIN/friday-daemon.exe"; CTL_BIN="$BIN/friday-ctl.exe"; HOST_BIN="$BIN/friday-windows-host.exe"
for f in "$DAEMON_BIN" "$CTL_BIN" "$HOST_BIN"; do
  [[ -x "$f" ]] || { echo "missing $f (this test is for the Windows install)" >&2; exit 2; }
done
PY=$(command -v python3 || command -v python)

W=$(mktemp -d)
FAKE="" DAEMON="" HOST=""
cleanup() { kill $DAEMON $FAKE $HOST 2>/dev/null || true; wait 2>/dev/null || true; }
trap cleanup EXIT
fails=0
pass() { echo "  ok   $*"; }
fail() { echo "  FAIL $*"; fails=$((fails + 1)); }

"$PY" tests/e2e/fake_model.py "$W/port" "$W/model.log" &
FAKE=$!
for _ in $(seq 300); do [[ -s "$W/port" ]] && break; sleep 0.1; done
[[ -s "$W/port" ]] || { echo "the fake model never started" >&2; exit 1; }
PORT=$(cat "$W/port")

# A session of its own, so a tray's own host is not the one that answers.
export FRIDAY_SESSION="e2e-$$"
"$HOST_BIN" >"$W/host.log" 2>&1 &
HOST=$!
for _ in $(seq 100); do grep -q "listening" "$W/host.log" 2>/dev/null && break; sleep 0.2; done
grep -q "listening" "$W/host.log" 2>/dev/null && pass "the Windows host is listening" \
  || { fail "the Windows host never listened"; cat "$W/host.log"; exit 1; }

offline=(env -u HTTPS_PROXY -u HTTP_PROXY -u https_proxy -u http_proxy -u ALL_PROXY -u all_proxy
  FRIDAY_STATE_FILE="$W/state.json" FRIDAY_MEMORY_DIR="$W/memory" FRIDAY_LISTEN=127.0.0.1:0
  LLAMACPP_ENABLED=1 LLAMACPP_BASE_URL="http://127.0.0.1:$PORT/v1")
"${offline[@]}" "$DAEMON_BIN" >"$W/daemon.log" 2>&1 &
DAEMON=$!
for _ in $(seq 120); do grep -q '"status": *"running"' "$W/state.json" 2>/dev/null && break; sleep 0.5; done
grep -q '"status": *"running"' "$W/state.json" 2>/dev/null && pass "daemon running" \
  || { fail "daemon never reported running"; tail -20 "$W/daemon.log"; exit 1; }

echo "desktop control"
out=$("${offline[@]}" timeout 120 "$CTL_BIN" goal --wait "focus a window" 2>&1) || true
if grep -q '"outcome": *"completed"' <<<"$out"; then pass "the goal completes"
else fail "the goal did not complete"; sed 's/^/       /' <<<"$out" | tail -10; fi
grep -q "You control the person's Windows desktop" "$W/model.log" 2>/dev/null \
  && pass "the model was given the Windows desktop instructions" \
  || fail "the model never saw the Windows desktop instructions"

audit="$W/actions.jsonl"
if [[ -s "$audit" ]] && "$PY" -c 'import json,sys
rows=[json.loads(l) for l in open(sys.argv[1]) if l.strip()]
sys.exit(0 if any(r.get("verb")=="windows" and r.get("ok") is True and r.get("risk")=="read" for r in rows) else 1)' "$audit"; then
  pass "the windows action reached the host and is in actions.jsonl as a successful read"
else
  fail "no successful windows action in actions.jsonl"; cat "$audit" 2>/dev/null | sed 's/^/       /' | head -5
  tail -15 "$W/daemon.log" | sed 's/^/       /'
fi
# The result of the first step the model took for a goal, as the model saw it.
first_step() { "$PY" -c 'import json,sys
for l in open(sys.argv[1], encoding="utf-8"):
    for line in json.loads(l).get("prompt", "").splitlines():
        if line.startswith("1. " + sys.argv[2] + " "): print(line); sys.exit(0)' "$W/model.log" "$1"; }
for case in "volume|turn the volume to 30|no audio output" "media|pause the music|Nothing is playing" "now_playing|what's playing|"; do
  IFS='|' read -r verb goal plain <<<"$case"
  out=$("${offline[@]}" timeout 120 "$CTL_BIN" goal --wait "$goal" 2>&1) || true
  grep -q '"outcome": *"completed"' <<<"$out" && pass "completes: $goal" || { fail "did not complete: $goal"; sed 's/^/       /' <<<"$out" | tail -6; }
  step=$(first_step "$verb")
  if [[ -z "$step" ]]; then fail "$verb: the model never got a result for it"
  elif grep -qiE "Exception|InvalidCast|Unknown:|COMException|0x[0-9A-F]{8}" <<<"$step"; then fail "$verb: the host leaked an error: ${step:0:200}"
  elif grep -q -- "-> ok: " <<<"$step" || { [[ -n "$plain" ]] && grep -qi -- "$plain" <<<"$step"; }; then pass "$verb: ${step:0:150}"
  else fail "$verb: neither a result nor the expected sentence: ${step:0:200}"; fi
done
if grep -qi "panicked" "$W/daemon.log"; then fail "the daemon panicked"; else pass "the daemon did not panic"; fi

echo
(( fails == 0 )) && echo "desktop control ok" || { echo "$fails problem(s)"; exit 1; }
