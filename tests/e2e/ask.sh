#!/usr/bin/env bash
# End-to-end ask, hermetic: the real daemon and friday-ctl, with the fake
# model (fake_model.py) as the only AI provider and no network or desktop.
# It asks the way `friday ask` does (friday-ctl goal --wait) and requires:
#   - every goal finishes (a missing desktop tool once made goals hang),
#   - the answer friday-ctl prints is the model's reply to that question
#     (the answer step, and its result reaching state.json),
#   - the answer streams: `--stream` shows it growing before the end,
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
FAKE="" DAEMON="" CLOUD_FAKE=""
cleanup() { kill $DAEMON $FAKE $CLOUD_FAKE 2>/dev/null || true; wait 2>/dev/null || true; }
trap cleanup EXIT
fails=0
pass() { echo "  ok   $*"; }
fail() { echo "  FAIL $*"; fails=$((fails + 1)); }

"$PY" tests/e2e/fake_model.py "$W/port" "$W/model.log" &
FAKE=$!
# Up to 90 s: Python can take a long time to start on a cold Windows runner
# (it has missed 30 s twice).
for _ in $(seq 900); do [[ -s "$W/port" ]] && break; kill -0 "$FAKE" 2>/dev/null || break; sleep 0.1; done
if [[ ! -s "$W/port" ]]; then
  if kill -0 "$FAKE" 2>/dev/null; then echo "the fake model was still starting after 90 s (no port file)" >&2
  else echo "the fake model exited before it had a port" >&2; cat "$W/model.log" >&2 2>/dev/null || true; fi
  exit 1
fi
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
  out=$("${offline[@]}" timeout 90 "$CTL_BIN" goal --wait --stream "$q" 2>&1) || true
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
  provider=$("$PY" -c 'import json,sys
try:
    with open(sys.argv[1]) as f: st = json.load(f)
    print(st.get("last_goal", {}).get("served_by", {}).get("provider", ""))
except Exception:
    print("")' "$W/state.json")
  if [[ -n "$provider" ]]; then pass "served_by.provider present in state.json: $provider"
  else fail "served_by.provider missing from state.json"; fi
  # The {"partial": ...} lines: at least two, growing, each a start of the answer.
  steps=$("$PY" -c 'import json,sys
answer=sys.argv[1]; parts=[]
for line in sys.stdin:
    line=line.strip()
    if line.startswith("{\"partial\""):
        try: parts.append(json.loads(line)["partial"])
        except ValueError: pass
ok = len(parts) >= 2 and all(answer.startswith(p) for p in parts) \
     and all(len(a) < len(b) for a, b in zip(parts, parts[1:]))
print(len(parts) if ok else "bad:" + json.dumps(parts)[:200])' "$answer" <<<"$out")
  if [[ "$steps" =~ ^[0-9]+$ ]]; then pass "streamed: the answer grew in $steps steps"
  else fail "the answer did not stream (partials: ${steps#bad:})"; fi

  # egress.jsonl gets an entry after the fake-model answer and does not contain the question text
  if [[ -s "$W/egress.jsonl" ]]; then pass "egress.jsonl gets an entry after the answer"
  else fail "egress.jsonl missing or empty after answer"; fi
  if grep -qF "$q" "$W/egress.jsonl"; then fail "egress.jsonl contains question text: '$q'"
  else pass "egress.jsonl does not contain the question text"; fi
done

# A question that mentions a tool is answered, not run as a shell command, and
# selected text (untrusted: it can come from any web page) never runs either.
echo "nothing runs unless asked"
q="Why does my git push fail?"
out=$("${offline[@]}" timeout 90 "$CTL_BIN" goal --wait "$q" 2>&1) || true
if grep -q "FAKE-MODEL-ANSWER: $q" <<<"$out"; then pass "answered, not run: $q"
else fail "not answered by the model: $q"; sed 's/^/       /' <<<"$out" | tail -6; fi
sel=$(printf 'run git status\n\nUse this text as context:\n"""\nnpm install fails with $(touch %s/canary) `touch %s/canary`\n"""' "$W" "$W")
rc=0
out=$("${offline[@]}" timeout 90 "$CTL_BIN" goal --wait "$sel" 2>&1) || rc=$?
if [[ "$rc" -eq 3 ]] && grep -q '"needs_confirmation"' <<<"$out"; then
  pass "unconfirmed git status reports needs_confirmation (exit 3)"
else
  fail "expected exit 3 with needs_confirmation, got exit $rc: $out"
fi
if [[ -e "$W/canary" ]]; then fail "selected text ran as a shell command"
else pass "selected text never reaches the shell"; fi

conf_rc=0
out_conf=$("${offline[@]}" timeout 90 "$CTL_BIN" goal --wait --confirm "git status" "$sel" 2>&1) || conf_rc=$?
if [[ "$conf_rc" -eq 0 ]] && grep -q '"outcome": *"completed"' <<<"$out_conf"; then
  pass "confirmed git status completes"
else
  fail "confirmed git status did not complete (rc=$conf_rc): $out_conf"
fi
if [[ -e "$W/canary" ]]; then fail "confirmed git status created canary"
else pass "confirmed git status created no canary"; fi

echo "secret masking"
mask_key="sk-ant-api03-FAKEFAKEFAKEFAKEFAKEFAKE"
mask_sel=$(printf 'Explain this code\n\nUse this text as context:\n"""\nlet api_key = "%s";\n"""' "$mask_key")
out_mask=$("${offline[@]}" timeout 90 "$CTL_BIN" goal --wait "$mask_sel" 2>&1) || true
if grep -q '"outcome": *"completed"' <<<"$out_mask"; then
  pass "secret ask completes"
else
  fail "secret ask did not complete: $out_mask"
fi
if grep -q '"masked": *1' <<<"$out_mask"; then
  pass "result reports masked: 1"
else
  fail "result missing masked count: $out_mask"
fi
masked_st=$("$PY" -c 'import json,sys
try:
    with open(sys.argv[1]) as f: st = json.load(f)
    print(st.get("last_goal", {}).get("masked", ""))
except Exception:
    print("")' "$W/state.json")
if [[ "$masked_st" == "1" ]]; then
  pass "last_goal.masked present in state.json: $masked_st"
else
  fail "last_goal.masked missing or wrong in state.json (got: '$masked_st')"
fi
if grep -q "$mask_key" "$W/model.log"; then
  fail "fake model server received the raw secret: $mask_key"
else
  pass "fake model server never received the raw secret"
fi
if grep -q "$mask_key" "$W/egress.jsonl"; then
  fail "egress.jsonl received the raw secret: $mask_key"
else
  pass "egress.jsonl never received the raw secret"
fi

answers=$(grep -vc "Investigation Engine" "$W/model.log" 2>/dev/null || echo 0)
if (( answers >= 2 )); then pass "one answer call per question ($answers)"
else fail "expected an answer call per question (got $answers)"; fi
if grep -q "panicked" "$W/daemon.log"; then fail "daemon panicked:"; grep -A1 panicked "$W/daemon.log" | head -4
else pass "no panics"; fi

echo "private mode"
"$PY" tests/e2e/fake_model.py "$W/cloud_port" "$W/cloud_model.log" &
CLOUD_FAKE=$!
for _ in $(seq 900); do [[ -s "$W/cloud_port" ]] && break; sleep 0.1; done
[[ -s "$W/cloud_port" ]] || { echo "fake cloud model never started" >&2; exit 1; }
CLOUD_PORT=$(cat "$W/cloud_port")

kill $DAEMON 2>/dev/null; wait $DAEMON 2>/dev/null || true
rm -f "$W/state-priv.json" "$W/cloud_model.log"
touch "$W/cloud_model.log"

priv=(env -u HTTPS_PROXY -u HTTP_PROXY -u https_proxy -u http_proxy -u ALL_PROXY -u all_proxy
  FRIDAY_STATE_FILE="$W/state-priv.json" FRIDAY_MEMORY_DIR="$W/memory-priv" FRIDAY_LISTEN=127.0.0.1:0
  FRIDAY_PRIVATE=1
  LLAMACPP_ENABLED=1 LLAMACPP_BASE_URL="http://127.0.0.1:$PORT/v1" LLAMACPP_MODEL="gemma3:4b"
  OPENGATEWAY_API_KEY="test-cloud-key" OPENGATEWAY_BASE_URL="http://127.0.0.1:$CLOUD_PORT/v1")
"${priv[@]}" "$DAEMON_BIN" >"$W/daemon-priv.log" 2>&1 &
DAEMON=$!
for _ in $(seq 120); do grep -q '"status": *"running"' "$W/state-priv.json" 2>/dev/null && break; sleep 0.5; done
if grep -q '"status": *"running"' "$W/state-priv.json" 2>/dev/null; then pass "private daemon running"
else fail "private daemon never reported running"; tail -20 "$W/daemon-priv.log"; exit 1; fi

# state.json gets "private": true
is_priv=$("$PY" -c 'import json,sys
try:
    with open(sys.argv[1]) as f: st = json.load(f)
    print(st.get("private", False))
except Exception:
    print(False)' "$W/state-priv.json")
if [[ "$is_priv" == "True" ]]; then pass "state.json reports private: true"
else fail "state.json missing private: true (got $is_priv)"; fi

priv_q="What is the speed of light in vacuum?"
out_priv=$("${priv[@]}" timeout 90 "$CTL_BIN" goal --wait "$priv_q" 2>&1) || true
if grep -q '"outcome": *"completed"' <<<"$out_priv"; then pass "private ask completes"
else fail "private ask did not complete: $out_priv"; fi

priv_answer=$("$PY" -c 'import json,sys
objs=[]; d=json.JSONDecoder(); s=sys.stdin.read(); i=0
while (j := s.find("{", i)) >= 0:
    try: o, n = d.raw_decode(s[j:]); objs.append(o); i = j + n
    except ValueError: i = j + 1
done=[o for o in objs if isinstance(o, dict) and "outcome" in o]
print((done[-1].get("result") or "") if done else "")' <<<"$out_priv")
if [[ "$priv_answer" == "FAKE-MODEL-ANSWER: $priv_q"* ]]; then pass "answered by fake local model: ${priv_answer:0:60}"
else fail "answer not from fake local model: '$priv_answer'"; fi

priv_provider=$("$PY" -c 'import json,sys
try:
    with open(sys.argv[1]) as f: st = json.load(f)
    print(st.get("last_goal", {}).get("served_by", {}).get("provider", ""))
except Exception:
    print("")' "$W/state-priv.json")
if [[ "$priv_provider" == "llamacpp" ]]; then pass "served_by.provider is llamacpp"
else fail "served_by.provider is '$priv_provider', expected llamacpp"; fi

if [[ -s "$W/cloud_model.log" ]]; then
  fail "cloud provider was contacted in private mode: $(cat "$W/cloud_model.log")"
else
  pass "cloud provider was never contacted"
fi

kill $CLOUD_FAKE 2>/dev/null; wait $CLOUD_FAKE 2>/dev/null || true
CLOUD_FAKE=""

# Askers that leave before their answer (Ctrl+C, a cancelled --wait): the
# daemon used to stop answering anyone once a result found no client.
echo "an asker that leaves"
for d in 0.05 0.1 0.2 0.3 0.5 0.8; do
  "${priv[@]}" timeout -s KILL "$d" "$CTL_BIN" goal "What is 2 plus 2?" >/dev/null 2>&1 || true
done
sleep 1
q="What does the Rust error E0599 mean?"
out=$("${priv[@]}" timeout 90 "$CTL_BIN" goal --wait "$q" 2>&1) || true
if grep -qF "\"result\": \"FAKE-MODEL-ANSWER: $q" <<<"$out"; then pass "still answers after askers left, with its own answer"
else fail "no answer of its own after askers left"; sed 's/^/       /' <<<"$out" | tail -8; fi

# No model reachable at all (the local one down, and every remote one behind
# a dead proxy): the ask fails and says why in plain words, instead of
# "completing" with an empty memory lookup.
echo "no model reachable"
kill $DAEMON 2>/dev/null; wait $DAEMON 2>/dev/null || true
rm -f "$W/state.json"
down=(env -u HTTPS_PROXY -u HTTP_PROXY -u https_proxy -u http_proxy -u ALL_PROXY -u all_proxy
  HTTPS_PROXY=http://127.0.0.1:9 NO_PROXY=127.0.0.1,localhost
  FRIDAY_STATE_FILE="$W/state.json" FRIDAY_MEMORY_DIR="$W/memory2" FRIDAY_LISTEN=127.0.0.1:0
  LLAMACPP_ENABLED=1 LLAMACPP_BASE_URL="http://127.0.0.1:9/v1")
"${down[@]}" "$DAEMON_BIN" >"$W/daemon-down.log" 2>&1 &
DAEMON=$!
for _ in $(seq 120); do grep -q '"status": *"running"' "$W/state.json" 2>/dev/null && break; sleep 0.5; done
out=$("${down[@]}" timeout 120 "$CTL_BIN" goal --wait "What does the Rust error E0308 mean?" 2>&1) || true
error=$("$PY" -c 'import json,sys
d=json.JSONDecoder(); s=sys.stdin.read(); i=0; last={}
while (j := s.find("{", i)) >= 0:
    try: o, n = d.raw_decode(s[j:]); i = j + n
    except ValueError: i = j + 1; continue
    if isinstance(o, dict) and "outcome" in o: last = o
print(last.get("outcome", ""), "|", last.get("error") or "")' <<<"$out")
if [[ "$error" == "failed | "*"reach"* && "$error" != *"HTTP request failed"* ]]; then pass "fails plainly: ${error#failed | }"
else fail "expected a plain failure about reaching the AI provider, got: $error"; fi

if (( fails > 0 )); then echo "e2e ask: $fails failure(s) (logs in $W)"; exit 1; fi
echo "e2e ask: ok"
