#!/usr/bin/env bash
# The `friday` CLI with its external tools stubbed: ask (+ selection, history,
# full multi-line answers), answers, setup-ai, welcome, doctor.
source "$(dirname "$0")/lib.sh"
need jq || { skip "friday CLI (needs jq)"; exit 0; }

F="$REPO_ROOT/packaging/linux/friday"
W="$WORK/cli"; S="$W/stub"; LOG="$W/calls.log"
rm -rf "$W"; mkdir -p "$S" "$W/home"
export HOME="$W/home" XDG_STATE_HOME="" XDG_CONFIG_HOME="" USER="${USER:-tester}" FRIDAY_WELCOME_DELAY=0
: >"$LOG"

cat >"$S/friday-ctl" <<EOF
#!/bin/bash
if [[ "\$1" == cancel ]]; then
  echo "friday-ctl-cancel" >> "$LOG"
  [[ -n "\$HAVE_GOAL" ]] && { echo '{"accepted":true}'; exit 0; }
  echo "no goal is running" >&2; exit 1
fi
if [[ "\$1" == context ]]; then
  printf 'friday-ctl-context %s\n' "\$*" >> "$LOG"
  printf '{\n "response": "The error says E0308.\\\\nFix the type."\n}\n'; exit 0
fi
printf 'friday-ctl %q\n' "\$3" >> "$LOG"
echo '{"accepted":true}'
if [[ "\$3" == *nodaemon* ]]; then echo "no token found; daemon not running at /x/state.json (is friday-linux-daemon up?)" >&2; exit 1; fi
if [[ "\$3" == *slowgoal* ]]; then echo '{"accepted":true}'; echo "timed out after 180s waiting for the goal to finish" >&2; exit 1; fi
if [[ "\$3" == *limit* ]]; then printf '{\n "outcome": "failed",\n "error": "provider opencode: 429 FreeUsageLimitError"\n}\n'; exit 1; fi
if [[ "\$3" == *fail* ]]; then printf '{\n "outcome": "failed",\n "error": "network unreachable"\n}\n'; exit 1; fi
printf '{\n "outcome": "completed",\n "result": "Line one.\\\\nLine two.\\\\nLast line."\n}\n'
EOF
cat >"$S/notify-send" <<EOF
#!/bin/bash
echo "notify \$*" >> "$LOG"
[[ "\$*" == *open=* ]] && echo open
[[ "\$*" == *setup=* ]] && echo setup
[[ "\$*" == *start=* ]] && echo start
true
EOF
printf '#!/bin/bash\necho "foot $*" >> %q\n' "$LOG" >"$S/foot"
printf '#!/bin/bash\necho "systemctl $*" >> %q\n[[ "$*" == *is-active* && -n "$STUB_INACTIVE" ]] && exit 3\nexit 0\n' "$LOG" >"$S/systemctl"
printf '#!/bin/bash\n[[ "$*" == *--primary* ]] && { printf %%s "$PRIMARY"; exit 0; }\nprintf %%s "$CLIP"\n' >"$S/wl-paste"
printf '#!/bin/bash\necho "fuzzel $*" >> %q\necho "$PICK"\n' "$LOG" >"$S/fuzzel"
printf '#!/bin/bash\necho "curl $*" >> %q\n[[ -n "$OFFLINE" ]] && exit 6\nexit 0\n' "$LOG" >"$S/curl"
chmod +x "$S"/*
export PATH="$S:$PATH"
state="$HOME/.local/state/friday"

echo "ask"
bash "$F" ask "what is my uptime"; sleep 0.3
contains "multi-line answer kept whole in the notification" "$(cat "$LOG")" $'Line one.\nLine two.\nLast line.'
contains "full answer saved" "$(cat "$state/last-answer.md")" "Last line."
contains "notification offers Open" "$(cat "$LOG")" "open=Open full answer"
contains "Open shows the answer" "$(cat "$LOG")" "foot -T FRIDAY · what is my uptime less -R"

PRIMARY=$'error: E0308 mismatched types' PICK="explain this" bash "$F" ask --selection; sleep 0.3
contains "selection sent as context" "$(cat "$LOG")" "Use this text as context"
contains "selection text included" "$(cat "$LOG")" "E0308 mismatched types"
PRIMARY="" CLIP="Bonjour" bash "$F" ask -s "translate"; sleep 0.3
contains "falls back to the clipboard" "$(cat "$LOG")" "Bonjour"
: >"$LOG"; PRIMARY="" CLIP="Bonjour tout le monde" PICK="translate" bash "$F" ask -s; sleep 0.3
contains "prompt says the text came from the clipboard" "$(cat "$LOG")" 'Clipboard: "Bonjour tout le monde'
: >"$LOG"; PRIMARY="" CLIP="hunter2SecretToken99" bash "$F" ask -s "explain"; sleep 0.3
if grep -q "friday-ctl" "$LOG"; then fail "a password-shaped clipboard was sent"
else pass "password-shaped clipboard is never sent"; fi
contains "and the user is told why" "$(cat "$LOG")" "looks like a password or token"
: >"$LOG"; PRIMARY="hunter2SecretToken99" PICK="explain" bash "$F" ask -s; sleep 0.3
contains "highlighted text is sent as chosen" "$(cat "$LOG")" "hunter2SecretToken99"
: >"$LOG"; PRIMARY="" CLIP="" bash "$F" ask -s
contains "empty selection explains what to do" "$(cat "$LOG")" "Select or copy some text first"
PICK="what does this error mean" bash "$F" ask --screen; sleep 0.3
contains "screen: asks the daemon with consent and the question" "$(cat "$LOG")" "friday-ctl-context context screen --yes --ask what does this error mean"
contains "screen: prompt says a screenshot is shared" "$(cat "$LOG")" "a screenshot goes to your AI provider"
contains "screen: full answer saved" "$(cat "$state/last-answer.md")" $'The error says E0308.\nFix the type.'
bash "$F" ask "hit the limit"; sleep 0.3
contains "free-limit error explained with a way out" "$(cat "$state/last-answer.md")" "free limit is used up for now"
: >"$LOG"; bash "$F" ask "nodaemon test"; sleep 0.5
contains "daemon down: plain message" "$(cat "$state/last-answer.md")" "FRIDAY isn't running"
contains "daemon down: raw error kept for debugging" "$(cat "$state/last-answer.md")" "no token found"
contains "daemon down: offers Start FRIDAY" "$(cat "$LOG")" "start=Start FRIDAY"
contains "Start FRIDAY restarts the daemon" "$(cat "$LOG")" "systemctl --user restart friday-linux-daemon"
bash "$F" ask "slowgoal test"; sleep 0.3
contains "timeout: explains it may still be running" "$(cat "$state/last-answer.md")" "taking longer than expected"
bash "$F" ask "this will fail"; sleep 0.3
contains "failure is critical with the error" "$(cat "$LOG")" "-u critical"
contains "failure shows the reason" "$(cat "$LOG")" "network unreachable"

history=$(cat "$state/ask-history")
if [[ "$(head -n1 <<<"$history")" == "this will fail" && "$history" != *Bonjour* ]]; then
  pass "history: newest first, questions only"
else fail "history order/content: $(tr '\n' '|' <<<"$history")"; fi
expect "friday answers --last prints the last answer" bash -c "bash '$F' answers --last | grep -q 'this will fail'"

echo "cancel"
: >"$LOG"
out=$(HAVE_GOAL=1 bash "$F" cancel 2>&1); rc=$?
[[ $rc == 0 && "$out" == *"Cancelled the current goal"* ]] && pass "cancels a running goal" || fail "cancel with a goal: rc=$rc $out"
contains "cancel goes through friday-ctl" "$(cat "$LOG")" "friday-ctl-cancel"
out=$(bash "$F" cancel 2>&1); rc=$?
[[ $rc == 1 && "$out" == *"no goal is running"* ]] && pass "says so when nothing is running" || fail "cancel without a goal: rc=$rc $out"

echo "setup-ai"
mkdir -p "$HOME/.config/friday"; printf 'FRIDAY_LOG=debug\nOLD_API_KEY=zzzzzzzzzz\n' >"$HOME/.config/friday/friday.env"
expect "accepts a key on stdin" bash -c "echo AIzaSyTestKey_12345 | bash '$F' setup-ai gemini"
envf="$HOME/.config/friday/friday.env"
if [[ "$(stat -c %a "$envf")" == 600 ]]; then pass "env file is 0600"; else fail "env file mode $(stat -c %a "$envf")"; fi
contains "keeps unrelated settings" "$(cat "$envf")" "FRIDAY_LOG=debug"
if grep -q OLD_API_KEY "$envf"; then fail "old key not removed"; else pass "old key removed"; fi
contains "vision provider enables screen context" "$(cat "$envf")" "FRIDAY_CONTEXT_PROVIDER=gemini"
contains "daemon restarted" "$(cat "$LOG")" "systemctl --user restart friday-linux-daemon"
refuse "rejects a malformed key" bash -c "echo 'bad key!' | bash '$F' setup-ai groq"
refuse "rejects an unknown provider" bash -c "echo AIzaSyTestKey_12345 | bash '$F' setup-ai openai"
expect "opencode works with no key" bash -c "printf '\\n' | bash '$F' setup-ai opencode"
if grep -qE '_API_KEY=' "$envf"; then fail "opencode without key left an API key in the env file"
else pass "switching to opencode free models clears the old key"; fi
contains "opencode handles screen questions" "$(cat "$envf")" "FRIDAY_CONTEXT_PROVIDER=opencode"

echo "welcome"
: >"$LOG"; rm -f "$envf"
bash "$F" welcome
contains "no key: still greets, mentions free models" "$(cat "$LOG")" "OpenCode Zen's free models"
contains "offers choosing a provider" "$(cat "$LOG")" "setup=Choose AI provider"
contains "setup opens in a terminal" "$(cat "$LOG")" "foot -T FRIDAY setup"
: >"$LOG"; bash "$F" welcome
if [[ ! -s "$LOG" ]]; then pass "shown only once"; else fail "shown twice"; fi

echo "doctor"
printf '{"status":"running","token":"s3cret-token"}' >"$state/state.json"
printf 'GEMINI_API_KEY=abc123secret\n' >"$envf"
out=$(bash "$F" doctor 2>&1 || true)
contains "reads daemon status" "$out" "state.json reports: running"
printf '{"status":"running","token":"t","listen":"127.0.0.1:1"}' >"$state/state.json"
out=$(bash "$F" doctor 2>&1 || true)
contains "probes the address the daemon published" "$out" "nothing listening on 127.0.0.1:1"
out=$(STUB_INACTIVE=1 bash "$F" doctor 2>&1 || true)
contains "stale state (running, but service dead) is flagged" "$out" "stale: the daemon crashed or was killed"
contains "names the key variable" "$out" "GEMINI_API_KEY is set"
contains "checks the configured provider is reachable" "$out" "generativelanguage.googleapis.com is reachable"
out=$(OFFLINE=1 bash "$F" doctor 2>&1 || true)
contains "offline: says the provider can't be reached" "$out" "can't reach generativelanguage.googleapis.com"
rm -f "$envf"; out=$(bash "$F" doctor 2>&1 || true)
contains "no key is fine: uses OpenCode Zen" "$out" "using OpenCode Zen's free models"
contains "no key: checks OpenCode Zen is reachable" "$out" "opencode.ai is reachable"
if [[ "$out" == *"!"*"API key"* ]]; then fail "doctor warns about a missing key"; else pass "doctor does not warn without a key"; fi
if [[ "$out" == *s3cret* || "$out" == *abc123secret* ]]; then fail "doctor printed a secret"; else pass "doctor prints no secrets"; fi
if [[ "$out" == *Quickshell* || "$out" == *quickshell* ]]; then fail "standalone (no ii shell): doctor checks the Quickshell bar"
else pass "standalone (no ii shell): no Quickshell checks"; fi
mkdir -p "$HOME/.config/quickshell/ii"; out=$(bash "$F" doctor 2>&1 || true)
contains "FRIDAY OS (ii shell): checks the bar" "$out" "Quickshell bar"

finish
