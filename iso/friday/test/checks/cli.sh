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
if [[ "\$*" == *"--confirm"* ]]; then
  printf 'friday-ctl-confirm %s\n' "\$*" >> "$LOG"
  echo '{"accepted":true}'
  printf '{\n "outcome": "completed",\n "result": "Confirmed and done."\n}\n'
  exit 0
fi
if [[ "\$*" == *gated* ]]; then
  echo '{"accepted":true}'
  printf '{\n "outcome": "failed",\n "error": "Confirm to run: git status",\n "needs_confirmation": {\n   "tool": "terminal",\n   "action": "git status"\n }\n}\n'
  exit 3
fi
printf 'friday-ctl %q\n' "\$3" >> "$LOG"
echo '{"accepted":true}'
if [[ "\$3" == *nodaemon* ]]; then echo "no token found; daemon not running at /x/state.json (is friday-linux-daemon up?)" >&2; exit 1; fi
if [[ "\$3" == *slowgoal* ]]; then echo '{"accepted":true}'; echo "timed out after 180s waiting for the goal to finish" >&2; exit 1; fi
if [[ "\$3" == *refused* ]]; then printf '{\n "outcome": "failed",\n "error": "Couldn'"'"'t reach your AI provider. Check your internet connection (or that your local model server is running)."\n}\n'; echo "llamacpp: tcp connect error: Connection refused" >&2; exit 1; fi
if [[ "\$3" == *limit* ]]; then printf '{\n "outcome": "failed",\n "error": "provider opencode: 429 FreeUsageLimitError"\n}\n'; exit 1; fi
if [[ "\$3" == *fail* ]]; then printf '{\n "outcome": "failed",\n "error": "network unreachable"\n}\n'; exit 1; fi
printf '{\n "outcome": "completed",\n "result": "Line one.\\\\nLine two.\\\\nLast line."\n}\n'
EOF
cat >"$S/notify-send" <<EOF
#!/bin/bash
echo "notify \$*" >> "$LOG"
if [[ "\$*" == *again=* && -n "\$ACTION_AGAIN" ]]; then
  if [[ ! -f "$W/again_done" ]]; then
    touch "$W/again_done"
    echo again
    exit 0
  fi
fi
[[ "\$*" == *copy=* && -n "\$ACTION_COPY" ]] && { echo copy; exit 0; }
[[ "\$*" == *open=* ]] && echo open
[[ "\$*" == *setup=* ]] && echo setup
[[ "\$*" == *start=* ]] && echo start
[[ "\$*" == *run=* && -z "\$CANCEL_CONFIRM" ]] && echo run
[[ "\$*" == *cancel=* && -n "\$CANCEL_CONFIRM" ]] && echo cancel
true
EOF
printf '#!/bin/bash\necho "wl-copy: $(cat)" >> %q\n' "$LOG" >"$S/wl-copy"
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
if [[ "$(stat -c %a "$state")" == 700 ]]; then pass "questions and answers are private (0700)"
else fail "state dir mode $(stat -c %a "$state")"; fi
contains "notification offers Open" "$(cat "$LOG")" "open=Open full answer"
contains "Open shows the answer" "$(cat "$LOG")" "foot -T FRIDAY · what is my uptime less -R"
contains "notification offers Copy" "$(cat "$LOG")" "copy=Copy"
contains "notification offers Ask again" "$(cat "$LOG")" "again=Ask again"

: >"$LOG"; ACTION_COPY=1 bash "$F" ask "copy answer test"; sleep 0.3
contains "Copy runs wl-copy with the answer text" "$(cat "$LOG")" "wl-copy: Line one."
if grep -q "wl-copy: # " "$LOG"; then fail "Copy included markdown header"; else pass "Copy copies answer body only"; fi

: >"$LOG"; rm -f "$W/again_done"
ACTION_AGAIN=1 PICK="re-asked question" bash "$F" ask "first question"; sleep 0.5
contains "Ask again re-invokes the ask" "$(cat "$LOG")" "friday-ctl re-asked\\ question"

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
: >"$LOG"; rm -f "$W/again_done"
PRIMARY="selection context text" ACTION_AGAIN=1 PICK="explain more" bash "$F" ask --selection; sleep 0.5
contains "Ask again: selection preserves --selection kind" "$(cat "$LOG")" "selection context text"
: >"$LOG"; PRIMARY="" CLIP="" bash "$F" ask -s
contains "empty selection explains what to do" "$(cat "$LOG")" "Select or copy some text first"
PICK="what does this error mean" bash "$F" ask --screen; sleep 0.3
contains "screen: asks the daemon with consent and the question" "$(cat "$LOG")" "friday-ctl-context context screen --yes --ask what does this error mean"
contains "screen: prompt says a screenshot is shared" "$(cat "$LOG")" "a screenshot goes to your AI provider"
contains "screen: full answer saved" "$(cat "$state/last-answer.md")" $'The error says E0308.\nFix the type.'
: >"$LOG"; rm -f "$W/again_done"
ACTION_AGAIN=1 PICK="screen follow-up" bash "$F" ask --screen; sleep 0.5
contains "Ask again: screen preserves --screen kind" "$(cat "$LOG")" "friday-ctl-context context screen --yes --ask screen follow-up"
bash "$F" ask "hit the limit"; sleep 0.3
contains "free-limit error explained with a way out" "$(cat "$state/last-answer.md")" "free limit is used up for now"
: >"$LOG"; bash "$F" ask "nodaemon test"; sleep 0.5
contains "daemon down: plain message" "$(cat "$state/last-answer.md")" "FRIDAY isn't running"
contains "daemon down: raw error kept for debugging" "$(cat "$state/last-answer.md")" "no token found"
contains "daemon down: offers Start FRIDAY" "$(cat "$LOG")" "start=Start FRIDAY"
contains "Start FRIDAY restarts the daemon" "$(cat "$LOG")" "systemctl --user restart friday-linux-daemon"
bash "$F" ask "slowgoal test"; sleep 0.3
contains "timeout: explains it may still be running" "$(cat "$state/last-answer.md")" "taking longer than expected"
: >"$LOG"; bash "$F" ask "model refused test"; sleep 0.3
contains "model server down: says so plainly" "$(cat "$state/last-answer.md")" "Couldn't reach your AI provider"
if grep -q "start=Start FRIDAY" "$LOG"; then fail "a model server refusing was taken for FRIDAY being down"
else pass "a model server refusing is not FRIDAY being down"; fi
bash "$F" ask "this will fail"; sleep 0.3
contains "failure is critical with the error" "$(cat "$LOG")" "-u critical"
contains "failure shows the reason" "$(cat "$LOG")" "network unreachable"
if grep -q "copy=Copy" "$LOG"; then fail "failure offered copy"; else pass "failure does not offer copy"; fi
if grep -q "again=Ask again" "$LOG"; then fail "failure offered ask again"; else pass "failure does not offer ask again"; fi

history=$(cat "$state/ask-history")
if [[ "$(head -n1 <<<"$history")" == "this will fail" && "$history" != *Bonjour* ]]; then
  pass "history: newest first, questions only"
else fail "history order/content: $(tr '\n' '|' <<<"$history")"; fi
expect "friday answers --last prints the last answer" bash -c "bash '$F' answers --last | grep -q 'this will fail'"

echo "confirm"
: >"$LOG"; bash "$F" ask "gated action"; sleep 0.4
contains "confirms before running action" "$(cat "$LOG")" "run=Run"
contains "confirm notification mentions exact action" "$(cat "$LOG")" "git status"
contains "run re-sends with confirm" "$(cat "$LOG")" "friday-ctl-confirm"
contains "confirmed answer saved" "$(cat "$state/last-answer.md")" "Confirmed and done."

: >"$LOG"; CANCEL_CONFIRM=1 bash "$F" ask "gated action"; sleep 0.4
contains "cancel option presented" "$(cat "$LOG")" "cancel=Cancel"
if grep -q "friday-ctl-confirm" "$LOG"; then fail "cancel still ran confirmed goal"
else pass "cancel does not re-send or run action"; fi

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
# --keep-others: the other providers' keys stay (FRIDAY can fall back to them);
# only this provider's own key is replaced.
printf 'FRIDAY_LOG=debug\nGEMINI_API_KEY=AIzaOldKey_12345\nGROQ_API_KEY=gsk_oldkey_123\nFRIDAY_CONTEXT_PROVIDER=gemini\n' >"$envf"
expect "--keep-others accepts a key" bash -c "echo gsk_newkey_456 | bash '$F' setup-ai --keep-others groq"
contains "--keep-others keeps the other provider's key" "$(cat "$envf")" "GEMINI_API_KEY=AIzaOldKey_12345"
contains "--keep-others saves the new key" "$(cat "$envf")" "GROQ_API_KEY=gsk_newkey_456"
if grep -q gsk_oldkey_123 "$envf"; then fail "--keep-others left the provider's old key"; else pass "--keep-others replaces that provider's own key"; fi
contains "--keep-others: a vision provider takes screen questions" "$(cat "$envf")" "FRIDAY_CONTEXT_PROVIDER=groq"
if [[ $(grep -c '^FRIDAY_CONTEXT_PROVIDER=' "$envf") == 1 ]]; then pass "one screen provider"; else fail "FRIDAY_CONTEXT_PROVIDER repeated"; fi
expect "--keep-others with a text-only provider" bash -c "echo sk-or-key_789 | bash '$F' setup-ai --keep-others openrouter"
contains "a text-only provider leaves screen questions where they were" "$(cat "$envf")" "FRIDAY_CONTEXT_PROVIDER=groq"
contains "all three keys kept" "$(grep -c '_API_KEY=' "$envf")" "3"
expect "--only replaces every key" bash -c "echo AIzaSyTestKey_12345 | bash '$F' setup-ai --only gemini"
if [[ $(grep -c '_API_KEY=' "$envf") == 1 ]]; then pass "--only leaves one key"; else fail "--only kept other keys"; fi

echo "welcome"
: >"$LOG"; rm -f "$envf"
bash "$F" welcome
contains "no key: still greets, mentions free models" "$(cat "$LOG")" "OpenCode Zen's free models"
contains "offers choosing a provider" "$(cat "$LOG")" "setup=Choose AI provider"
contains "setup opens in a terminal" "$(cat "$LOG")" "foot -T FRIDAY setup"
: >"$LOG"; bash "$F" welcome
if [[ ! -s "$LOG" ]]; then pass "shown only once"; else fail "shown twice"; fi

echo "shell supervision"
# A stub quickshell: `-n -p DIR` starts it (crashing the first $QS_CRASHES
# times), `kill` ends the running one cleanly, anything else is IPC.
SH="$W/shell"; mkdir -p "$SH/run"; touch "$SH/shell.qml" "$SH/run/wayland-test"
cat >"$S/quickshell" <<STUB
#!/bin/bash
case "\$1" in
  kill) [[ -f "$SH/qs.pid" ]] && kill "\$(cat "$SH/qs.pid")" 2>/dev/null; exit 0 ;;
  -n) ;;
  *) echo "qs-ipc \$*" >> "$LOG"; exit 0 ;;
esac
n=\$(( \$(cat "$SH/count" 2>/dev/null || echo 0) + 1 )); echo \$n >"$SH/count"
echo "qs-start \$n" >> "$LOG"
(( n <= \${QS_CRASHES:-0} )) && exit 1
echo \$\$ >"$SH/qs.pid"
trap 'exit 0' TERM
sleep 30 & wait
STUB
chmod +x "$S/quickshell"
sh_env=(env FRIDAY_SHELL_DIR="$SH" XDG_RUNTIME_DIR="$SH/run" WAYLAND_DISPLAY=wayland-test FRIDAY_SHELL_BACKOFF=0)
waitfor() { local i; for i in $(seq 50); do eval "$1" && return 0; sleep 0.1; done; return 1; }
started() { [[ "$(cat "$SH/count" 2>/dev/null)" == "$1" ]] && [[ -f "$SH/qs.pid" ]] && kill -0 "$(cat "$SH/qs.pid")" 2>/dev/null; }

: >"$LOG"; rm -f "$SH/count" "$SH/qs.pid"
QS_CRASHES=2 "${sh_env[@]}" bash "$F" shell start 2>"$SH/err" &
SUP=$!
if waitfor "started 3"; then pass "restarts after crashes (2 crashes, 3rd start runs)"; else fail "did not restart after a crash"; fi
contains "says why it restarted" "$(cat "$SH/err")" "exited with status 1; restarting"
out=$("${sh_env[@]}" bash "$F" shell start 2>&1)
contains "a second start is refused" "$out" "already running"
"${sh_env[@]}" bash "$F" shell restart
if waitfor "started 4"; then pass "restart: the supervisor starts it again"; else fail "restart did not bring it back"; fi
qs_pid=$(cat "$SH/qs.pid")
"${sh_env[@]}" bash "$F" shell stop
if waitfor "! kill -0 $SUP 2>/dev/null"; then pass "stop ends the supervisor"; else fail "supervisor still running after stop"; kill "$SUP"; fi
if [[ ! -e "$SH/run/friday-shell-$(id -u).pid" ]]; then pass "stop removes the pid file"; else fail "pid file left behind"; fi
if waitfor "! kill -0 $qs_pid 2>/dev/null"; then pass "stop ends the shell"; else fail "shell still running after stop"; fi

: >"$LOG"; rm -f "$SH/count" "$SH/qs.pid"
QS_CRASHES=99 timeout 20 "${sh_env[@]}" bash "$F" shell start 2>/dev/null
contains "a crash loop stops after 6 starts" "$(grep -c qs-start "$LOG")" "6"
contains "and says so in a notification" "$(cat "$LOG")" "FRIDAY Shell keeps crashing"

: >"$LOG"; rm -f "$SH/count" "$SH/qs.pid"
QS_CRASHES=1 timeout 20 env FRIDAY_SHELL_DIR="$SH" XDG_RUNTIME_DIR="$SH/run" WAYLAND_DISPLAY=gone FRIDAY_SHELL_BACKOFF=0 \
  bash "$F" shell start 2>/dev/null
contains "no restart once the session is gone" "$(grep -c qs-start "$LOG")" "1"
"${sh_env[@]}" bash "$F" shell control
contains "drawer names go to IPC" "$(cat "$LOG")" "qs-ipc -p $SH ipc call shell toggle control"
conf="$HOME/.config/friday/shell.json"; rm -f "$conf"
"${sh_env[@]}" bash "$F" shell style island >/dev/null
"${sh_env[@]}" bash "$F" shell palette dawn >/dev/null
if [[ "$(jq -c . "$conf" 2>/dev/null)" == '{"style":"island","palette":"dawn"}' ]]; then pass "style and palette merge into shell.json"
else fail "shell.json after style/palette: $(cat "$conf" 2>/dev/null)"; fi
contains "style lists the choices, marking the current one" "$("${sh_env[@]}" bash "$F" shell style)" "* island"
if "${sh_env[@]}" bash "$F" shell style neon >/dev/null 2>&1; then fail "an unknown style was accepted"
else pass "an unknown style is refused"; fi
echo '{ "palette": "friday", "clock24h": false }' >"$conf"
"${sh_env[@]}" bash "$F" shell style dock >/dev/null
contains "other settings are kept" "$(cat "$conf")" '"clock24h": false'
contains "the old \"friday\" palette lists as ember" "$("${sh_env[@]}" bash "$F" shell palette)" "* ember"
look="$HOME/.config/friday/hypr-look.lua"
printf '#!/bin/bash\necho "hyprctl $*" >> %q\n' "$LOG" >"$S/hyprctl"; chmod +x "$S/hyprctl"
: >"$LOG"; HYPRLAND_INSTANCE_SIGNATURE=test "${sh_env[@]}" bash "$F" shell palette frost >/dev/null
contains "windows follow the style (dock gaps)" "$(cat "$look")" "gaps_out = 16"
contains "window borders follow the palette (frost accent)" "$(cat "$look")" 'active_border = "rgba(7cc4ffee)"'
contains "Hyprland reloads to show it" "$(cat "$LOG")" "hyprctl reload"
contains "niri's windows get the look too (dock gaps)" "$(cat "$HOME/.config/friday/niri-look.kdl")" "gaps 16"
"${sh_env[@]}" bash "$F" shell palette omarchy >/dev/null
if grep -q active_border "$look"; then fail "the omarchy palette still overrides Omarchy's border colours"
else pass "the omarchy palette leaves border colours to Omarchy's theme"; fi
rm -f "$S/hyprctl"
rm -f "$S/quickshell"

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
if [[ "$out" == *"FRIDAY Shell"* ]]; then fail "standalone: doctor checks FRIDAY Shell"
else pass "standalone: no FRIDAY Shell checks"; fi
mkdir -p "$HOME/.config/hypr"; echo 'o.exec_on_start("friday shell")' >"$HOME/.config/hypr/autostart.lua"
out=$(bash "$F" doctor 2>&1 || true)
contains "FRIDAY OS (FRIDAY Shell): says when it is not running" "$out" "FRIDAY Shell is not running"
contains "FRIDAY OS (FRIDAY Shell): suggests restarting it" "$out" "friday shell restart"
rm -rf "$HOME/.config/hypr"
mkdir -p "$HOME/.config/quickshell/ii"; out=$(bash "$F" doctor 2>&1 || true)
contains "FRIDAY OS (ii shell): checks the bar" "$out" "Quickshell bar"

finish
