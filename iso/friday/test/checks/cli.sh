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
target_arg="\$3"
for arg in "\$@"; do
  if [[ "\$arg" != "goal" && "\$arg" != "--wait" && "\$arg" != "--web" && "\$arg" != "--stream" && "\$arg" != friday* ]]; then
    target_arg="\$arg"
  fi
done
printf 'friday-ctl %q\n' "\$target_arg" >> "$LOG"
if [[ "\$*" == *"--web"* ]]; then
  printf 'friday-ctl-web %s\n' "\$*" >> "$LOG"
fi
echo '{"accepted":true}'
if [[ "\$target_arg" == *nodaemon* ]]; then echo "no token found; daemon not running at /x/state.json (is friday-linux-daemon up?)" >&2; exit 1; fi
if [[ "\$target_arg" == *slowgoal* ]]; then echo '{"accepted":true}'; echo "timed out after 180s waiting for the goal to finish" >&2; exit 1; fi
if [[ "\$target_arg" == *refused* ]]; then printf '{\n "outcome": "failed",\n "error": "Couldn'"'"'t reach your AI provider. Check your internet connection (or that your local model server is running)."\n}\n'; echo "llamacpp: tcp connect error: Connection refused" >&2; exit 1; fi
if [[ "\$target_arg" == *limit* ]]; then printf '{\n "outcome": "failed",\n "error": "provider opencode: 429 FreeUsageLimitError"\n}\n'; exit 1; fi
if [[ "\$target_arg" == *bullets* ]]; then printf '{\n "outcome": "completed",\n "result": "- first point\\\\n- second point"\n}\n'; exit 0; fi
if [[ "\$target_arg" == *fail* ]]; then printf '{\n "outcome": "failed",\n "error": "network unreachable"\n}\n'; exit 1; fi
printf '{\n "outcome": "completed",\n "result": "Line one.\\\\nLine two.\\\\nLast line."\n}\n'
EOF
cat >"$S/notify-send" <<EOF
#!/bin/bash
echo "notify \$*" >> "$LOG"
# Like the real one: text that starts with "-" before "--" is an unknown option.
args=("\$@"); i=0
while (( i < \${#args[@]} )); do
  case "\${args[i]}" in
    --) break ;;
    -a|-u|-t|-r|-A|-i|-c|-h|-e) i=\$((i + 2)) ;;
    -p|--print-id|-w|--wait) i=\$((i + 1)) ;;
    -*) echo "Unknown option \${args[i]}" >&2; exit 1 ;;
    *) i=\$((i + 1)) ;;
  esac
done
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
: >"$LOG"; bash "$F" ask "list some bullets"; sleep 0.3
contains "an answer that starts with '- ' is still notified" "$(cat "$LOG")" "-- FRIDAY ✓ - first point"
if grep -q "Unknown option" "$W"/*.log 2>/dev/null; then fail "notify-send rejected an answer"; else pass "no notify-send option errors"; fi

: >"$LOG"; ACTION_COPY=1 bash "$F" ask "copy answer test"; sleep 0.3
contains "Copy runs wl-copy with the answer text" "$(cat "$LOG")" "wl-copy: Line one."
if grep -q "wl-copy: # " "$LOG"; then fail "Copy included markdown header"; else pass "Copy copies answer body only"; fi

: >"$LOG"; bash "$F" ask --web "what is the latest stable rust"; sleep 0.3
contains "ask --web passes --web flag" "$(cat "$LOG")" "friday-ctl-web"
contains "ask --web passes the question" "$(cat "$LOG")" "what is the latest stable rust"

: >"$LOG"; bash "$F" ask "search what is the latest stable rust"; sleep 0.3
contains "ask 'search q' passes --web flag" "$(cat "$LOG")" "friday-ctl-web"
contains "ask 'search q' strips prefix" "$(cat "$LOG")" "what is the latest stable rust"
if grep -q "search what is" "$LOG"; then fail "prefix was not stripped"; else pass "prefix was stripped"; fi

: >"$LOG"; bash "$F" ask "search the web for rust releases"; sleep 0.3
contains "ask 'search the web for q' passes --web flag" "$(cat "$LOG")" "friday-ctl-web"
contains "ask 'search the web for q' strips prefix" "$(cat "$LOG")" "rust releases"

: >"$LOG"; bash "$F" ask "look up rust documentation"; sleep 0.3
contains "ask 'look up q' passes --web flag" "$(cat "$LOG")" "friday-ctl-web"
contains "ask 'look up q' strips prefix" "$(cat "$LOG")" "rust documentation"

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
echo "region ask"
printf '#!/bin/sh\n[ -n "$SLURP_CANCEL" ] && exit 1\necho "10,20 300x200"\n' >"$S/slurp"; chmod +x "$S/slurp"
: >"$LOG"; PICK="what is this chart" bash "$F" ask --region; sleep 0.3
contains "region: the picked area goes with the question" "$(cat "$LOG")" "context screen --yes --ask what is this chart --region 10,20 300x200"
: >"$LOG"; SLURP_CANCEL=1 PICK="never asked" bash "$F" ask --region; sleep 0.3
if grep -q "never asked" "$LOG"; then fail "region: Esc while dragging still asked"; else pass "region: Esc while dragging sends nothing"; fi
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
contains "each saved answer opens with a marker line (history stays whole)" "$(cat "$state/answers.md")" "<!-- friday answer -->"
printf '# what is a monad\n\n_2026-11-14 22:00_\n\nA monad wraps a value with a bind operation.\n' >"$state/last-answer.md"
: >"$LOG"; bash "$F" ask --follow-up "give an example"; sleep 0.3
contains "follow-up: the question is asked" "$(cat "$LOG")" "give an example"
contains "follow-up: the previous answer goes with it" "$(cat "$LOG")" "A monad wraps a value"

echo "sent"
rm -f "$state/egress.jsonl"
out=$(bash "$F" sent 2>&1 || true)
contains "empty case says nothing sent yet" "$out" "Nothing has been sent yet."

now_sec=$(date +%s)
ts1=$(date -u -d "@$(( now_sec - 3600 ))" "+%Y-%m-%dT%H:%M:%SZ")
ts2=$(date -u -d "@$(( now_sec - 7200 ))" "+%Y-%m-%dT%H:%M:%SZ")
ts3=$(date -u -d "@$(( now_sec - 10800 ))" "+%Y-%m-%dT%H:%M:%SZ")
cat >"$state/egress.jsonl" <<EOF
{"ts":"$ts1","provider":"opencode","model":"m1","kinds":["text"],"text_chars":1240,"image_bytes":0,"masked":0,"outcome":"ok"}
{"ts":"$ts2","provider":"opencode","model":"m2","kinds":["text","image"],"text_chars":310,"image_bytes":186368,"masked":1,"outcome":"ok"}
{"ts":"$ts3","provider":"kilo","model":"m3","kinds":["text"],"text_chars":500,"image_bytes":0,"masked":4,"outcome":"error"}
EOF

out=$(bash "$F" sent 2>&1 || true)
contains "table header present" "$out" "PROVIDER"
contains "table maps opencode to OpenCode Zen" "$out" "OpenCode Zen"
contains "table formats text only" "$out" "text 1,240 chars"
contains "table formats text + screenshot" "$out" "text 310 chars + screenshot 182 KB"
contains "table outcome present" "$out" "ok"
contains "summary includes request count and 7 days" "$out" "3 requests in the last 7 days: 2 OpenCode Zen, 1 Kilo"
contains "summary includes screenshot count" "$out" "1 with a screenshot"
contains "summary includes masked secrets count" "$out" "5 secrets masked"

out1=$(bash "$F" sent 1 2>&1 || true)
contains "limit N prints last entry" "$out1" "Kilo"
if grep -q "1,240 chars" <<<"$out1"; then fail "sent 1 printed older entries"; else pass "sent 1 limits table to last entry"; fi

echo "local commands (no AI)"
printf '#!/bin/sh\necho "wpctl $*" >>"%s"\n' "$LOG" >"$S/wpctl"; chmod +x "$S/wpctl"
: >"$LOG"; bash "$F" ask "Volume 40"; sleep 0.3
contains "volume: set on this computer" "$(cat "$LOG")" "wpctl set-volume @DEFAULT_AUDIO_SINK@ 40%"
contains "volume: says what it did" "$(cat "$state/last-answer.md")" "Volume set to 40%."
contains "local command says nothing was sent" "$(cat "$LOG")" "nothing was sent"
if grep -q "friday-ctl" "$LOG"; then fail "a local command went to the daemon"; else pass "a local command never reaches the daemon"; fi
: >"$LOG"; bash "$F" ask "timer 1 min"; sleep 0.3
contains "timer: set" "$(cat "$state/last-answer.md")" "Timer set for 1 minute."
: >"$LOG"; bash "$F" ask "what is the volume of a sphere"; sleep 0.3
contains "a real question about volume still goes to the model" "$(cat "$LOG")" "friday-ctl"

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

echo "voice (hold Super+Y)"
export XDG_RUNTIME_DIR="$W/run"; mkdir -p "$XDG_RUNTIME_DIR"
contains "voice status says it's off" "$(bash "$F" voice status)" "friday voice setup"
: >"$LOG"; PICK="typed after a tap" bash "$F" hold start; PICK="typed after a tap" bash "$F" hold end; sleep 0.3
contains "voice off: a tap opens the prompt and asks" "$(cat "$LOG")" "typed after a tap"
# Voice on: a stand-in recorder (records until interrupted) and whisper.cpp.
mkdir -p "$HOME/.local/share/friday/whisper"; head -c 100 /dev/zero >"$HOME/.local/share/friday/whisper/ggml-base.en.bin"
printf '#!/bin/bash\necho "pw-record $*" >>"%s"\ntrap "exit 0" INT TERM\n: >"${@: -1}"\nwhile :; do sleep 0.05; done\n' "$LOG" >"$S/pw-record"
printf '#!/bin/bash\n[ -n "$WHISPER_SILENT" ] && exit 0\necho " what time is it in Tokyo"\n' >"$S/whisper-cli"
chmod +x "$S/pw-record" "$S/whisper-cli"
contains "voice status says it's on" "$(bash "$F" voice status)" "hold Super+Y and speak"
: >"$LOG"; PICK="tapped with voice on" bash "$F" hold start; PICK="tapped with voice on" bash "$F" hold end; sleep 0.3
contains "voice on: a quick tap still opens the prompt" "$(cat "$LOG")" "tapped with voice on"
if grep -q "pw-record" "$LOG"; then fail "a quick tap started recording"; else pass "a quick tap records nothing"; fi
: >"$LOG"; bash "$F" hold start; sleep 0.9; bash "$F" hold end; sleep 0.3
contains "hold: records 16 kHz mono" "$(cat "$LOG")" "pw-record --rate 16000 --channels 1"
contains "hold: the spoken words are asked" "$(cat "$LOG")" "what time is it in Tokyo"
if pgrep -f "$S/pw-record" >/dev/null; then fail "the recorder kept running after the release"; else pass "the recorder stops on release"; fi
: >"$LOG"; bash "$F" hold start; sleep 0.9; WHISPER_SILENT=1 bash "$F" hold end; sleep 0.3
contains "hold with no words: says so" "$(cat "$LOG")" "I didn't catch that"
if grep -q "friday-ctl" "$LOG"; then fail "an empty recording was asked"; else pass "an empty recording asks nothing"; fi
rm -f "$HOME/.local/share/friday/whisper/ggml-base.en.bin"

echo "face unlock"
contains "face status says it's off and how to turn it on" "$(bash "$F" face status 2>&1)" "friday face setup"
expect "face setup changes nothing when declined" bash -c "printf 'n\\n' | bash '$F' face setup | grep -q 'Nothing changed'"
expect "face rejects an unknown action" bash -c "! bash '$F' face bogus"

echo "connect an account"
# A stub Claude Code: answers only once signed in (its login makes the mark).
cat >"$S/claude" <<EOF
#!/bin/bash
echo "claude \$*" >>"$LOG"
case "\$1" in
  auth) [ "\$2" = login ] && touch "$W/claude-signed-in"; exit 0 ;;
  -p) [ -e "$W/claude-signed-in" ] && echo ready; exit 0 ;;
esac
EOF
chmod +x "$S/claude"
: >"$LOG"; rm -f "$W/claude-signed-in"
expect "connect signs in when needed, then checks it answers" bash "$F" connect claude
contains "it ran the tool's own sign-in" "$(cat "$LOG")" "claude auth login"
contains "it asked headless, with no tools" "$(cat "$LOG")" "claude -p Reply with the single word: ready --output-format text"
contains "connected accounts are saved" "$(cat "$envf")" "FRIDAY_AGENT_CLIS=claude"
if grep -q '^FRIDAY_PREFER_ACCOUNTS=' "$envf"; then fail "an account was preferred without --prefer"; else pass "by default an account is a fallback after the free models"; fi
contains "connect says it's a fallback" "$(bash "$F" connect claude 2>&1)" "when the free models can't answer"
expect "connect --prefer" bash "$F" connect --prefer claude
contains "--prefer puts accounts first" "$(cat "$envf")" "FRIDAY_PREFER_ACCOUNTS=1"
: >"$LOG"
expect "connecting again needs no sign-in" bash "$F" connect claude
if grep -q "auth login" "$LOG"; then fail "signed in twice"; else pass "an account that answers isn't signed in again"; fi
if [[ $(grep -c '^FRIDAY_AGENT_CLIS=' "$envf") == 1 && "$(grep '^FRIDAY_AGENT_CLIS=' "$envf")" == "FRIDAY_AGENT_CLIS=claude" ]]; then pass "no duplicate entry"; else fail "FRIDAY_AGENT_CLIS: $(grep '^FRIDAY_AGENT_CLIS=' "$envf")"; fi
refuse "a missing tool says how to install it" bash "$F" connect codex
contains "install hint" "$(bash "$F" connect codex 2>&1)" "npm install -g @openai/codex"
refuse "an unknown account is refused" bash "$F" connect bard
expect "disconnect" bash "$F" disconnect claude
if grep -q '^FRIDAY_AGENT_CLIS=' "$envf"; then fail "disconnect left FRIDAY_AGENT_CLIS"; else pass "disconnect removes it"; fi
contains "keys survive connect/disconnect" "$(cat "$envf")" "_API_KEY="
rm -f "$S/claude"

echo "welcome"
: >"$LOG"; rm -f "$envf"
bash "$F" welcome
contains "no key: still greets, mentions free models" "$(cat "$LOG")" "free models (OpenCode Zen, then Kilo)"
contains "the welcome says free models may keep what's sent" "$(cat "$LOG")" "may keep what you send"
contains "offers connecting an account" "$(cat "$LOG")" "connect=Connect an account"
contains "offers choosing a provider" "$(cat "$LOG")" "setup=Choose AI provider"
contains "setup opens in a terminal" "$(cat "$LOG")" "foot -T FRIDAY setup"
: >"$LOG"; bash "$F" welcome
if [[ ! -s "$LOG" ]]; then pass "shown only once"; else fail "shown twice"; fi
# With FRIDAY Shell running, the welcome is its tour instead of a notification.
TOUR="$W/tour"; mkdir -p "$TOUR/bin" "$TOUR/shell"; : >"$TOUR/shell/shell.qml"
printf '#!/bin/bash\necho "quickshell $*" >>"%s"\n' "$LOG" >"$TOUR/bin/quickshell"; chmod +x "$TOUR/bin/quickshell"
: >"$LOG"; PATH="$TOUR/bin:$PATH" FRIDAY_SHELL_DIR="$TOUR/shell" bash "$F" welcome --force
contains "with FRIDAY Shell, the welcome starts its tour" "$(cat "$LOG")" "ipc call tour begin"
if grep -q "^notify " "$LOG"; then fail "the tour welcome also notified"; else pass "the tour replaces the welcome notification"; fi

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
contains "doctor reports configured search backends" "$out" "search backends: DuckDuckGo, Wikipedia"
printf 'BRAVE_SEARCH_API_KEY=testbravesecret123\nFRIDAY_SEARXNG_URL=http://localhost:8888\n' >>"$envf"
out=$(bash "$F" doctor 2>&1 || true)
contains "doctor reports SearXNG and Brave in search backends" "$out" "search backends: SearXNG, Brave, DuckDuckGo, Wikipedia"
if [[ "$out" == *testbravesecret123* ]]; then fail "doctor printed search key"; else pass "doctor never prints search keys"; fi
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

echo "report"
mkdir -p "$HOME/.local/state/friday" "$HOME/.config/friday"
printf '{"status":"running","last_goal":{"goal":"my private question","result":"my private answer","outcome":"completed","served_by":{"provider":"kilo","model":"m","ms":900}}}' \
  >"$HOME/.local/state/friday/state.json"
printf 'OPENCODE_API_KEY=s3cretvalue123456\nFRIDAY_AGENT_CLIS=agy\n' >"$HOME/.config/friday/friday.env"
cat >"$S/journalctl" <<STUBLOG
#!/bin/bash
cat <<'LOG'
daemon: provider error: Authorization: Bearer abcdefabcdef1234567890
daemon: request api_key=sk-live-0123456789abcdefXYZ failed
daemon: github token ghp_abcdefghijklmnopqrstuvwxyz0123456789 in env
daemon: connect 192.0.2.44:443 from fe80::5054:ff:fe12:3456 mac 52:54:00:12:34:56
daemon: mail person@example.com, file $HOME/notes/todo.txt
daemon: listening on 127.0.0.1:18432 at 12:34:56
-----BEGIN OPENSSH PRIVATE KEY-----
b3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQ
-----END OPENSSH PRIVATE KEY-----
LOG
STUBLOG
chmod +x "$S/journalctl"
out=$(bash "$F" report "$W/report.txt" 2>&1 || true)
contains "report: says where it wrote" "$out" "Wrote $W/report.txt" || echo "$out" | head -n 20
rep=$(cat "$W/report.txt" 2>/dev/null)
for sec in Versions Session Doctor "AI providers" "Daemon state" "Daemon log"; do
  contains "report: has the $sec section" "$rep" "== $sec"
done
for leak in s3cretvalue123456 abcdefabcdef1234567890 sk-live-0123456789abcdefXYZ ghp_abcdefghijklmnopqrstuvwxyz \
            192.0.2.44 fe80::5054 52:54:00:12:34:56 person@example.com "$HOME/notes" b3BlbnNzaC1rZXkt \
            "my private question" "my private answer"; do
  if [[ "$rep" == *"$leak"* ]]; then fail "report: leaks '$leak'"; else pass "report: no '${leak:0:24}'"; fi
done
contains "report: names the configured key, not its value" "$rep" "key set: OPENCODE_API_KEY"
contains "report: keeps who answered" "$rep" '"provider": "kilo"'
contains "report: keeps loopback addresses" "$rep" "127.0.0.1:18432"
contains "report: keeps timestamps" "$rep" "12:34:56"
contains "report: private keys are replaced" "$rep" "<private key redacted>"
contains "report: home path becomes ~" "$rep" "~/notes/todo.txt"
mode=$(stat -c %a "$W/report.txt" 2>/dev/null)
[[ "$mode" == 600 ]] && pass "report: written 0600" || fail "report: mode is $mode, not 600"

finish
