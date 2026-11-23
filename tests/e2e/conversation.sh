#!/usr/bin/env bash
# End-to-end conversation, hermetic (the real daemon and friday-ctl, the fake
# model as the only provider). Asks carry on one conversation:
#   - a later ask reaches the model with the earlier turn in its context,
#   - so does an ask after the daemon restarts,
#   - --new-conversation starts clean (no earlier turns).
#
# Usage: tests/e2e/conversation.sh [bin-dir]   (default target/debug)
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
for _ in $(seq 300); do [[ -s "$W/port" ]] && break; sleep 0.1; done
[[ -s "$W/port" ]] || { echo "the fake model never started" >&2; exit 1; }
offline=(env -u HTTPS_PROXY -u HTTP_PROXY -u https_proxy -u http_proxy -u ALL_PROXY -u all_proxy
  FRIDAY_STATE_FILE="$W/state.json" FRIDAY_MEMORY_DIR="$W/memory" FRIDAY_LISTEN=127.0.0.1:0
  XDG_CONFIG_HOME="$W/config" APPDATA="$W/appdata"
  LLAMACPP_ENABLED=1 LLAMACPP_BASE_URL="http://127.0.0.1:$(cat "$W/port")/v1")

start_daemon() {
  rm -f "$W/state.json"
  "${offline[@]}" "$DAEMON_BIN" >>"$W/daemon.log" 2>&1 &
  DAEMON=$!
  for _ in $(seq 120); do grep -q '"status": *"running"' "$W/state.json" 2>/dev/null && break; sleep 0.5; done
  grep -q '"status": *"running"' "$W/state.json" 2>/dev/null || { fail "daemon never reported running"; tail -20 "$W/daemon.log"; exit 1; }
}
# ask <question> [friday-ctl flags...]: ask, then print what the model last received.
ask() {
  local q=$1; shift
  : >"$W/model.log"
  "${offline[@]}" timeout 90 "$CTL_BIN" goal --wait "$@" "$q" >"$W/out" 2>&1 || true
  grep -q '"outcome": *"completed"' "$W/out" || { fail "did not complete: $q"; tail -5 "$W/out"; }
  "$PY" -c 'import json,sys
lines=[json.loads(l) for l in open(sys.argv[1]) if l.strip()]
print("\n".join(l.get("prompt") or "" for l in lines))' "$W/model.log"
}

start_daemon
echo "one conversation"
ask "Remember that the code word is heron." >/dev/null
seen=$(ask "What is the code word?")
if grep -q "\[session context\]" <<<"$seen" && grep -q "heron" <<<"$seen"; then
  pass "the next ask carries the earlier turn"
else fail "the next ask lost the earlier turn"; fi
# "Remember that …" also lands in the memory file the person can edit, and
# every ask carries it.
if grep -qs "the code word is heron" "$W/config/friday/memory.md" "$W/appdata/friday/memory.md"; then
  pass "it is kept in the memory file"
else fail "it isn't in the memory file"; fi
if grep -q "about you: the code word is heron" <<<"$seen"; then pass "and the next ask carries it"
else fail "the next ask doesn't carry the memory file"; fi
# Another assistant's notes come in only after a preview.
printf '# CLAUDE.md\n- I live in Chennai\n- the code word is heron\n```\n- not code\n```\n' >"$W/notes.md"
mem="$W/config/friday/memory.md"; [[ -f "$mem" ]] || mem="$W/appdata/friday/memory.md"
before=$(cat "$mem")
preview=$("${offline[@]}" "$CTL_BIN" memory import "$W/notes.md" 2>&1)
if grep -q "Would add 1 line" <<<"$preview" && grep -q "I live in Chennai" <<<"$preview" && [[ "$(cat "$mem")" == "$before" ]]; then
  pass "an import shows what it would add and adds nothing yet"
else fail "the import preview: $preview"; fi
"${offline[@]}" "$CTL_BIN" memory import "$W/notes.md" --apply >/dev/null 2>&1
if grep -q "^- I live in Chennai" "$mem" && [[ $(grep -c heron "$mem") == 1 ]] && ! grep -q "not code" "$mem"; then
  pass "--apply adds only the new line"
else fail "--apply: $(cat "$mem")"; fi

echo "after a restart"
kill $DAEMON; wait $DAEMON 2>/dev/null || true
if grep -q heron "$W/conversations.json" 2>/dev/null; then pass "the conversation was saved"
else fail "no saved conversation in $W/conversations.json"; fi
start_daemon
seen=$(ask "Say the code word once more.")
if grep -q "\[session context\]" <<<"$seen" && grep -q "heron" <<<"$seen"; then
  pass "the conversation carries on after the daemon restarts"
else fail "the restart lost the conversation"; fi

echo "a new conversation"
seen=$(ask "What did we just talk about?" --new-conversation)
if grep -q "\[session context\]" <<<"$seen"; then fail "--new-conversation still carried earlier turns"
else pass "--new-conversation starts clean"; fi

if (( fails > 0 )); then echo "e2e conversation: $fails failure(s) (logs in $W)"; exit 1; fi
echo "e2e conversation: ok"
