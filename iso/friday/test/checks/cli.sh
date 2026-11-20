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
if [[ "\$1" == say ]]; then
  if [[ "\$2" == "-" ]]; then text=\$(cat); else text="\${*:2}"; fi
  printf 'friday-ctl-say %s\n' "\$text" >> "$LOG"
  exit 0
fi
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
if [[ "\$*" == *"--thorough"* ]]; then
  printf 'friday-ctl-thorough %s\n' "\$*" >> "$LOG"
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
printf '#!/bin/bash\necho "sudo $*" >> %q\n"$@"\n' "$LOG" >"$S/sudo"
printf '#!/bin/bash\necho "pacman $*" >> %q\nexit 0\n' "$LOG" >"$S/pacman"
printf '#!/bin/bash\necho "ollama $*" >> %q\nexit 0\n' "$LOG" >"$S/ollama"
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

: >"$LOG"; bash "$F" ask --another opencode/big-pickle "explain E0308"; sleep 0.3
contains "ask --another asks for a thorough answer" "$(cat "$LOG")" "friday-ctl-thorough"
contains "ask --another names the model to skip" "$(cat "$LOG")" "--avoid opencode/big-pickle"
contains "ask --another still asks the question" "$(cat "$LOG")" "explain E0308"

: >"$LOG"; bash "$F" ask "explain E0308"; sleep 0.3
if grep -q "friday-ctl-thorough" "$LOG"; then fail "a plain ask went thorough"; else pass "a plain ask is not thorough"; fi

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
# The app the selected text came from rides along; off means it doesn't.
printf '#!/bin/bash\n[[ "$*" == "activewindow -j" ]] && printf %%s %q\n' '{"class":"firefox","title":"Rust \"E0308\" - docs"}' >"$S/hyprctl"; chmod +x "$S/hyprctl"
: >"$LOG"; PRIMARY="some text" PICK="explain" bash "$F" ask --selection; sleep 0.3
contains "selection names the app it came from" "$(cat "$LOG")" 'The text came from the app "firefox"'
contains "and the window title, without quotes in it" "$(cat "$LOG")" 'window title: "Rust E0308 - docs"'
: >"$LOG"; bash "$F" ask "what is rust"; sleep 0.3
if grep -q "The text came from" "$LOG"; then fail "a typed question named an app"; else pass "a typed question names no app"; fi
bash "$F" app-context off >/dev/null
: >"$LOG"; PRIMARY="some text" PICK="explain" bash "$F" ask --selection; sleep 0.3
if grep -q "The text came from" "$LOG"; then fail "app-context off still named the app"; else pass "app-context off sends no app name"; fi
contains "app-context status says off" "$(bash "$F" app-context status)" "Off"
bash "$F" app-context on >/dev/null
contains "app-context status says on" "$(bash "$F" app-context status)" "On"
rm -f "$S/hyprctl"
# `friday forget` shows counts, asks before deleting, and wipes only what was named.
FD="$HOME/.local/state/friday"; mkdir -p "$FD/memory"
printf '<!-- friday answer -->\n# q1\n\nA\n\n---\n\n<!-- friday answer -->\n# q2\n\nB\n\n---\n\n' >"$FD/answers.md"
printf 'q2\nq1\n' >"$FD/ask-history"; printf '{"a":1}\n{"a":2}\n{"a":3}\n' >"$FD/egress.jsonl"
printf '{"e":1}\n' >"$FD/memory/episodes.jsonl"
printf '%s\n' '{"ts":"2026-11-15T08:12:03.123+00:00","verb":"move_window","params":{"target":"firefox","workspace":"2"},"risk":"reversible","ok":true,"goal":"put firefox on workspace 2"}' \
  '{"ts":"2026-11-15T08:12:04+00:00","verb":"close_window","params":{"target":"gimp"},"risk":"destructive","ok":false,"goal":"close gimp"}' >"$FD/actions.jsonl"
out=$(bash "$F" forget </dev/null)
contains "forget lists the desktop actions" "$out" "2 things FRIDAY did on your desktop"
contains "forget lists the saved answers" "$out" "2 saved answers, 2 remembered questions"
contains "forget lists the sent log" "$out" "3 entries in the log"
contains "forget lists the memory" "$out" "1 things FRIDAY remembered"
if bash "$F" forget answers </dev/null >/dev/null 2>&1; then fail "forget deleted without a yes"; else pass "forget wants --yes when it can't ask"; fi
[[ -f "$FD/answers.md" ]] && pass "and nothing was deleted" || fail "answers.md vanished without a yes"
contains "forget answers reports counts" "$(bash "$F" forget answers --yes)" "Deleted 2 saved answers and 2 remembered questions"
[[ ! -e "$FD/answers.md" && ! -e "$FD/ask-history" ]] && pass "forget answers deletes them" || fail "forget answers left files"
[[ -f "$FD/egress.jsonl" && -f "$FD/memory/episodes.jsonl" ]] && pass "and only them" || fail "forget answers deleted more than asked"
bash "$F" forget all --yes >/dev/null
[[ ! -e "$FD/egress.jsonl" && ! -e "$FD/memory/episodes.jsonl" ]] && pass "forget all deletes the rest" || fail "forget all left files"
if bash "$F" forget nonsense >/dev/null 2>&1; then fail "forget took an unknown thing"; else pass "forget refuses an unknown thing"; fi

# `friday actions`: what FRIDAY did on the desktop, readable, newest last.
printf '%s\n' '{"ts":"2026-11-15T08:12:03.123+00:00","verb":"move_window","params":{"target":"firefox","workspace":"2"},"risk":"reversible","ok":true,"goal":"put firefox on workspace 2"}' \
  '{"ts":"2026-11-15T08:12:04+00:00","verb":"close_window","params":{"target":"gimp"},"risk":"destructive","ok":false,"goal":"close gimp"}' >"$FD/actions.jsonl"
acts=$(bash "$F" actions)
contains "actions shows the verb and its parameters" "$acts" 'move_window  {"target":"firefox","workspace":"2"}  (reversible)'
contains "actions marks a failure" "$acts" "failed"
contains "actions shows the time without fractions" "$acts" "2026-11-15 08:12:03"
[[ $(bash "$F" actions 1 | wc -l) == 1 ]] && pass "actions N limits to the last N" || fail "actions 1 printed more than one line"
contains "actions 1 is the newest" "$(bash "$F" actions 1)" "close_window"

# `friday tasks`: standing tasks, listed and edited without the daemon.
contains "tasks with none says how to make one" "$(bash "$F" tasks)" "every morning at 8"
printf '%s\n' '{"tasks":[{"id":"t1","goal":"tell me the weather","trigger":{"kind":"daily","hour":8,"minute":5},"created":1},{"id":"t2","goal":"say hi","trigger":{"kind":"battery_below","percent":20},"created":1}]}' >"$FD/standing.json"
tl=$(bash "$F" tasks)
contains "tasks lists a daily task" "$tl" "t1  every day at 08:05  tell me the weather"
contains "tasks lists a battery task" "$tl" "t2  battery under 20%  say hi"
bash "$F" tasks pause t1 >/dev/null
contains "tasks pause marks it" "$(bash "$F" tasks)" "(paused)"
bash "$F" tasks resume t1 >/dev/null
[[ "$(bash "$F" tasks)" != *"(paused)"* ]] && pass "tasks resume clears it" || fail "tasks resume left it paused"
bash "$F" tasks delete t2 >/dev/null
[[ "$(bash "$F" tasks --json | jq length)" == 1 ]] && pass "tasks delete removes only that task" || fail "tasks delete removed the wrong number"
bash "$F" tasks delete t9 >/dev/null 2>&1 && fail "tasks delete of a missing id succeeded" || pass "tasks delete of a missing id fails"
bash "$F" tasks delete 'x; rm' >/dev/null 2>&1 && fail "tasks delete accepted a bad id" || pass "tasks delete refuses a bad id"
[[ "$(stat -c %a "$FD/standing.json")" == 600 ]] && pass "tasks keeps the file private" || fail "standing.json is not 0600"
rm -f "$FD/standing.json"

# `friday agents`: coding-agent runs, listed, shown and answered without the daemon.
contains "agents with none says how to start one" "$(bash "$F" agents)" "have opencode add a README"
mkdir -p "$FD/agents"
printf '%s\n' '{"id":"r1a2b3","agent":"opencode","folder":"/w/site","request":"add a README","task":"Add a README.","status":"waiting_for_you","started":1,"pending":{"step":"git push","why":"runs `git push`"},"summary":"","approved_by_friday":2,"approved_by_you":0,"rejected":0}' >"$FD/agents/r1a2b3.json"
printf '%s\n' '{"event":"approved","by":"friday","step":"cargo test","ts":1}' >"$FD/agents/r1a2b3.jsonl"
al=$(bash "$F" agents)
contains "agents lists a waiting run" "$al" "r1a2b3  opencode  /w/site  WAITING FOR YOU: git push"
contains "agents says how to answer" "$al" "friday agents allow <id>"
contains "agents show reports who approved what" "$(bash "$F" agents show r1a2b3)" "approved by friday: cargo test"
bash "$F" agents deny r1a2b3 >/dev/null
[[ "$(cat "$FD/agents/r1a2b3.answer")" == deny ]] && pass "agents deny answers the run" || fail "agents deny didn't write the answer"
bash "$F" agents allow 'x; rm' >/dev/null 2>&1 && fail "agents allow accepted a bad id" || pass "agents allow refuses a bad id"
bash "$F" agents reply r1a2b3 use the MIT license >/dev/null
[[ "$(cat "$FD/agents/r1a2b3.answer")" == "reply:use the MIT license" ]] && pass "agents reply passes the answer on" || fail "agents reply didn't write the answer"
rm -rf "$FD/agents"
if bash "$F" actions oops >/dev/null 2>&1; then fail "actions took a non-number"; else pass "actions refuses a non-number"; fi
contains "forget actions reports counts" "$(bash "$F" forget actions --yes)" "Deleted 2 entries from the log of desktop actions"
[[ ! -e "$FD/actions.jsonl" ]] && pass "forget actions deletes the log" || fail "forget actions left the log"
contains "actions with no log says so" "$(bash "$F" actions)" "hasn't done anything on your desktop yet"

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
# Asking about the screen in words looks at it; a how-to about the screen does not.
: >"$LOG"; bash "$F" ask "what is on my screen right now" >/dev/null 2>&1; sleep 0.4
contains "screen by words: 'what is on my screen' looks at the screen" "$(cat "$LOG")" "context screen --yes --ask what is on my screen right now"
: >"$LOG"; bash "$F" ask "how do I change my screen brightness" >/dev/null 2>&1; sleep 0.4
if grep -q "context screen" "$LOG"; then fail "a how-to about the screen captured it"; else pass "a how-to about the screen does not capture it"; fi
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

# The launcher's quick commands (shell/services/Commands.qml) promise "nothing sent":
# each phrase it hands to `friday ask` must be one the CLI answers itself. (dnd and
# wifi settings need the running shell, so the shell check covers those.)
while IFS= read -r phrase; do
  : >"$LOG"; bash "$F" ask "$phrase" >/dev/null 2>&1; sleep 0.3
  if grep -q "friday-ctl" "$LOG"; then fail "launcher command '$phrase' goes to the AI, not handled locally"
  else pass "launcher command '$phrase' is handled locally"; fi
done < <(grep -o 'run: \["friday", "ask", "[^"]*"\]' "$REPO_ROOT/packaging/linux/shell/services/Commands.qml" \
         | sed 's/.*"ask", "\([^"]*\)".*/\1/' | grep -viE '^(dnd|wifi|how )')

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
: >"$LOG"; PICK="toggled with voice off" bash "$F" hold toggle; sleep 0.3
contains "toggle with voice off is a plain ask" "$(cat "$LOG")" "toggled with voice off"
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
# (The tap just above is followed at once by more key presses: its half-second timer must not start a recording for them.)
# Voice typing (hold Super+Shift+Y): the same recording, but the words are typed where you are, not asked.
printf '#!/bin/bash\necho "wtype $*" >>"%s"\n' "$LOG" >"$S/wtype"; chmod +x "$S/wtype"
: >"$LOG"; bash "$F" hold start --type; sleep 0.9; bash "$F" hold end --type; sleep 0.5
contains "voice typing: the spoken words are typed" "$(cat "$LOG")" "wtype -- what time is it in Tokyo"
if grep -q "friday-ctl" "$LOG"; then fail "voice typing sent the words to the AI"; else pass "voice typing asks nothing"; fi
: >"$LOG"; bash "$F" hold start --type; sleep 0.9; WHISPER_SILENT=1 bash "$F" hold end --type; sleep 0.3
contains "voice typing with no words: says so" "$(cat "$LOG")" "Super+Shift+Y"
if grep -q "wtype" "$LOG"; then fail "voice typing typed when nothing was said"; else pass "voice typing types nothing when nothing was said"; fi
: >"$LOG"; bash "$F" hold start --type; bash "$F" hold end --type; sleep 0.3
if grep -qE "pw-record|wtype|friday-ctl" "$LOG"; then fail "a quick tap on the voice typing key did something"; else pass "a quick tap on the voice typing key does nothing"; fi
if grep -q "friday-ctl" "$LOG"; then fail "an empty recording was asked"; else pass "an empty recording asks nothing"; fi
# niri has no release bind: one press listens, the next one asks.
: >"$LOG"; bash "$F" hold toggle; sleep 0.5
contains "toggle: the first press records" "$(cat "$LOG")" "pw-record --rate 16000 --channels 1"
if grep -q "friday-ctl" "$LOG"; then fail "the first toggle press asked"; else pass "the first toggle press only listens"; fi
bash "$F" hold toggle; sleep 0.3
contains "toggle: the second press asks the spoken words" "$(cat "$LOG")" "what time is it in Tokyo"
if pgrep -f "$S/pw-record" >/dev/null; then fail "the recorder kept running after the second press"; else pass "the recorder stops on the second press"; fi
rm -f "$HOME/.local/share/friday/whisper/ggml-base.en.bin"

echo "face unlock"
contains "face status says it's off and how to turn it on" "$(bash "$F" face status 2>&1)" "friday face setup"
expect "face setup changes nothing when declined" bash -c "printf 'n\\n' | bash '$F' face setup | grep -q 'Nothing changed'"
expect "face rejects an unknown action" bash -c "! bash '$F' face bogus"

echo "screen-text"
contains "screen-text is off by default" "$(bash "$F" screen-text status)" "Screen text: off"
contains "screen-text on asks first and declining changes nothing" "$(printf 'n\n' | bash "$F" screen-text on 2>&1)" "Nothing changed"
contains "screen-text still off after declining" "$(bash "$F" screen-text status)" "Screen text: off"
bash "$F" screen-text on --yes >/dev/null
contains "screen-text on is saved" "$(bash "$F" screen-text status)" "Screen text: on"
[[ "$(stat -c %a "$HOME/.config/friday/friday.env")" == 600 ]] && pass "screen-text keeps the env file private" || fail "friday.env is not 0600"
bash "$F" screen-text off >/dev/null
contains "screen-text off is saved" "$(bash "$F" screen-text status)" "Screen text: off"
[[ "$(grep -c '^FRIDAY_SCREEN_TEXT=' "$HOME/.config/friday/friday.env" 2>/dev/null || true)" == 0 ]] && pass "screen-text off leaves no setting" || fail "FRIDAY_SCREEN_TEXT left in friday.env"

echo "desktop control and standing tasks switches"
contains "desktop control is on by default" "$(bash "$F" desktop status)" "Desktop control: on"
bash "$F" desktop off >/dev/null
contains "desktop off is saved" "$(bash "$F" desktop status)" "Desktop control: off"
contains "desktop off writes FRIDAY_DESKTOP=0" "$(cat "$HOME/.config/friday/friday.env")" "FRIDAY_DESKTOP=0"
[[ "$(stat -c %a "$HOME/.config/friday/friday.env")" == 600 ]] && pass "desktop switch keeps the env file private" || fail "friday.env is not 0600"
bash "$F" desktop on >/dev/null
contains "desktop on is saved" "$(bash "$F" desktop status)" "Desktop control: on"
[[ "$(grep -c '^FRIDAY_DESKTOP=' "$HOME/.config/friday/friday.env" 2>/dev/null || true)" == 0 ]] && pass "desktop on leaves no setting" || fail "FRIDAY_DESKTOP left in friday.env"
expect "desktop rejects an unknown action" bash -c "! bash '$F' desktop bogus 2>/dev/null"
contains "standing tasks are on by default" "$(bash "$F" tasks status)" "Standing tasks: on"
bash "$F" tasks off >/dev/null
contains "tasks off is saved" "$(bash "$F" tasks status)" "Standing tasks: off"
contains "tasks off writes FRIDAY_STANDING_TASKS=0" "$(cat "$HOME/.config/friday/friday.env")" "FRIDAY_STANDING_TASKS=0"
bash "$F" tasks on >/dev/null
contains "tasks on is saved" "$(bash "$F" tasks status)" "Standing tasks: on"

echo "keys update (FRIDAY OS homes made before a key was added)"
mkdir -p "$HOME/.config/hypr"
grep -v -- '--type' "$REPO_ROOT/iso/friday/airootfs/etc/skel/.config/hypr/bindings.lua" >"$HOME/.config/hypr/bindings.lua"
cp "$HOME/.config/hypr/bindings.lua" "$W/old-keys.lua"
contains "doctor-style check finds the missing voice typing key" "$(grep -c -- '--type' "$HOME/.config/hypr/bindings.lua" || true)" "0"
contains "keys update adds voice typing" "$(bash "$F" keys update)" "Added voice typing"
[[ "$(grep -c -- '--type' "$HOME/.config/hypr/bindings.lua")" == 2 && "$(grep -v -- '--type' "$HOME/.config/hypr/bindings.lua")" == "$(cat "$W/old-keys.lua")" ]] \
  && pass "keys update only adds the two voice typing lines" || fail "keys update changed more than it should"
contains "keys update a second time changes nothing" "$(bash "$F" keys update)" "up to date"
rm -rf "$HOME/.config/hypr"
contains "keys update without a FRIDAY OS key file does nothing" "$(bash "$F" keys update)" "nothing to update"

echo "phone"
# `friday phone`, without Pocketdesk installed.
if ! command -v pocketdesk >/dev/null 2>&1; then
  contains "phone status says Pocketdesk is not installed" "$(bash "$F" phone status 2>&1)" "not installed"
  expect "phone on needs Pocketdesk" bash -c "! bash '$F' phone on </dev/null"
fi
# With a stub Pocketdesk: declining changes nothing.
CS="$W/confirm-stub"; mkdir -p "$CS"
printf '#!/bin/sh\nexit 0\n' >"$CS/pocketdesk"; chmod +x "$CS/pocketdesk"
contains "phone on asks first and declining changes nothing" "$(printf 'n\n' | PATH="$CS:$PATH" bash "$F" phone on 2>&1)" "Nothing changed"
rm -f "$CS/pocketdesk"
# A confirmation goes to the screen and, with a paired phone set to answer, the
# phone too: the first answer wins; no answer is a cancel.
eval "$(sed -n '/^friday_confirm() {/,/^}/p' "$F")"
cat >"$CS/notify-send" <<'EOF2'
#!/bin/sh
# NS_ANSWER after NS_DELAY seconds, as a clicked action prints its key.
sleep "${NS_DELAY:-0}"
[ -z "${NS_ANSWER:-}" ] || echo "$NS_ANSWER"
EOF2
cat >"$CS/pocketdesk" <<'EOF2'
#!/bin/sh
# `pocketdesk ask`: PD_ANSWER (yes|no|none) after PD_DELAY seconds; the question is logged.
echo "$*" >>"${PD_LOG:-/dev/null}"
sleep "${PD_DELAY:-0}"
case "${PD_ANSWER:-none}" in yes) echo yes; exit 0 ;; no) echo no; exit 1 ;; *) exit 2 ;; esac
EOF2
chmod +x "$CS/notify-send" "$CS/pocketdesk"
conf() { PATH="$CS:$PATH" friday_confirm "git push origin main" ""; }
export PD_LOG="$W/pd.log"; : >"$PD_LOG"
[[ "$(NS_ANSWER=run PD_ANSWER=none conf)" == run ]] && pass "confirm: the screen's Run runs it (phone has no answer)" || fail "confirm: screen Run"
[[ "$(NS_ANSWER=cancel PD_ANSWER=none conf)" == cancel ]] && pass "confirm: the screen's Cancel cancels" || fail "confirm: screen Cancel"
start=$SECONDS; r=$(NS_ANSWER=run NS_DELAY=5 PD_ANSWER=yes conf)
[[ "$r" == run && $((SECONDS - start)) -lt 4 ]] && pass "confirm: the phone's Yes runs it without waiting for the screen" || fail "confirm: phone Yes ($r after $((SECONDS - start))s)"
[[ "$(NS_ANSWER=run NS_DELAY=5 PD_ANSWER=no conf)" == cancel ]] && pass "confirm: the phone's No cancels" || fail "confirm: phone No"
[[ "$(NS_ANSWER= PD_ANSWER=none conf)" == cancel ]] && pass "confirm: no answer anywhere is a cancel" || fail "confirm: no answer"
contains "confirm: the phone is asked the exact action" "$(cat "$PD_LOG")" "FRIDAY wants to run: git push origin main"
# `friday phone status` counts the paired phones from `pocketdesk devices --json`.
printf '#!/bin/sh\necho "{\\"api\\":1,\\"devices\\":[{\\"id\\":\\"a\\"},{\\"id\\":\\"b\\"}],\\"server_running\\":true}"\n' >"$CS/pocketdesk"; chmod +x "$CS/pocketdesk"
printf '#!/bin/sh\nexit 0\n' >"$CS/systemctl"; chmod +x "$CS/systemctl"
contains "phone status counts the paired phones" "$(PATH="$CS:$PATH" bash "$F" phone status)" "on, 2 paired"
rm -f "$CS/systemctl"
rm -f "$CS/pocketdesk"
[[ "$(NS_ANSWER=run conf)" == run ]] && pass "confirm: works with no Pocketdesk at all" || fail "confirm: no Pocketdesk"
unset PD_LOG
echo "helper"
contains "help lists helper" "$(bash "$F" help 2>&1)" "helper"
expect "helper status exits 0" bash "$F" helper status
h_status=$(bash "$F" helper status 2>&1)
if [[ "$h_status" =~ ^System\ helper: ]]; then
  pass "helper status prints a line starting with System helper:"
else
  fail "helper status did not start with System helper: ($h_status)"
fi
if [[ -f /usr/lib/friday/friday-os-helper ]]; then
  contains "helper status reports off when package is installed" "$h_status" "System helper: off"
fi

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

echo "private mode"
: >"$LOG"
mkdir -p "$(dirname "$envf")"
printf 'FRIDAY_LOG=debug\nOTHER_VAR=keepme\n' >"$envf"
chmod 600 "$envf"

out=$(bash "$F" private on 2>&1)
contains "explains answers stay on computer" "$out" "Answers stay on this computer"
contains "explains slower than cloud" "$out" "Slower than cloud models"
contains "explains disk space needed" "$out" "Needs ~3–5 GB of disk space"
contains "says private mode is on" "$out" "Private mode is on: gemma3:4b on this computer."
contains "pacman installed ollama" "$(cat "$LOG")" "pacman -S --needed --noconfirm ollama"
contains "ollama service enabled" "$(cat "$LOG")" "systemctl enable --now ollama"
contains "pulled model" "$(cat "$LOG")" "ollama pull gemma3:4b"
contains "daemon restarted" "$(cat "$LOG")" "systemctl --user restart friday-linux-daemon"
mode=$(stat -c %a "$envf")
[[ "$mode" == 600 ]] && pass "env file mode 0600 after private on" || fail "env file mode is $mode, not 600"
contains "private on sets FRIDAY_PRIVATE" "$(cat "$envf")" "FRIDAY_PRIVATE=1"
contains "private on sets LLAMACPP_ENABLED" "$(cat "$envf")" "LLAMACPP_ENABLED=1"
contains "private on sets LLAMACPP_BASE_URL" "$(cat "$envf")" "LLAMACPP_BASE_URL=http://127.0.0.1:11434/v1"
contains "private on sets LLAMACPP_MODEL" "$(cat "$envf")" "LLAMACPP_MODEL=gemma3:4b"
contains "private on sets LLAMACPP_VISION" "$(cat "$envf")" "LLAMACPP_VISION=1"
contains "keeps other lines" "$(cat "$envf")" "FRIDAY_LOG=debug"
contains "keeps other vars" "$(cat "$envf")" "OTHER_VAR=keepme"

# Status reports on, model, and ollama running
out_status=$(bash "$F" private status 2>&1)
contains "status reports on" "$out_status" "Private mode: on"
contains "status reports model" "$out_status" "Model: gemma3:4b"
contains "status reports ollama" "$out_status" "Ollama: running"

# In private mode, friday ask notification meta says "Private · on this computer"
: >"$LOG"
bash "$F" ask "private ask test"; sleep 0.3
contains "notification says Private · on this computer" "$(cat "$LOG")" "Private · on this computer"
if grep -q "via llamacpp" "$LOG"; then fail "notification included via line in private mode"; else pass "no via line in private mode"; fi

# Turn private mode off
out_off=$(bash "$F" private off 2>&1)
contains "says private mode is off" "$out_off" "Private mode is off."
contains "says ollama stays installed" "$out_off" "Ollama stays installed (sudo pacman -Rns ollama to remove it, ollama rm gemma3:4b to free space)."
if grep -qE '^(FRIDAY_PRIVATE|LLAMACPP_ENABLED|LLAMACPP_BASE_URL|LLAMACPP_MODEL|LLAMACPP_VISION)=' "$envf"; then
  fail "private off left private lines in friday.env"
else
  pass "private off removed exactly the 5 lines"
fi
contains "private off keeps other lines" "$(cat "$envf")" "FRIDAY_LOG=debug"
contains "private off keeps other vars" "$(cat "$envf")" "OTHER_VAR=keepme"
mode_off=$(stat -c %a "$envf")
[[ "$mode_off" == 600 ]] && pass "env file mode 0600 after private off" || fail "env file mode is $mode_off, not 600"

out_status_off=$(bash "$F" private status 2>&1)
contains "status reports off" "$out_status_off" "Private mode: off"

echo "welcome"
: >"$LOG"; rm -f "$envf"
bash "$F" welcome
contains "no key: still greets, mentions free models" "$(cat "$LOG")" "free models (OpenCode Zen, then Kilo or OVHcloud)"
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

echo "spoken answers (friday voice speak)"
VD="$HOME/.local/share/friday/voice"; SPK="$HOME/.config/friday/speak"
contains "speak status: not installed" "$(bash "$F" voice speak status)" "voice speak setup"
if bash "$F" voice speak on >/dev/null 2>&1; then fail "spoken answers turned on without a voice"; else pass "no spoken answers without a voice"; fi
mkdir -p "$VD/bin" "$VD/model"; printf '#!/bin/sh\n' >"$VD/bin/sherpa-onnx-offline-tts"; chmod +x "$VD/bin/sherpa-onnx-offline-tts"
echo '{"kind":"supertonic","speaker":0}' >"$VD/settings.json"
contains "installed but off by default" "$(bash "$F" voice speak status)" "installed, off"
: >"$LOG"; bash "$F" ask "what is rust" >/dev/null 2>&1; sleep 0.4
if grep -q "friday-ctl-say" "$LOG"; then fail "an answer was spoken while spoken answers are off"; else pass "answers stay silent while it is off"; fi
bash "$F" voice speak on >/dev/null
contains "on" "$(bash "$F" voice speak status)" "Spoken answers: on"
: >"$LOG"; bash "$F" ask "what is rust" >/dev/null 2>&1; sleep 0.6
contains "an answer is spoken when it is on" "$(cat "$LOG")" "friday-ctl-say"
bash "$F" voice speak voice 4 >/dev/null
contains "the chosen voice is kept" "$(cat "$VD/settings.json")" '"speaker": 4'
if bash "$F" voice speak voice loud >/dev/null 2>&1; then fail "a voice that isn't a number was accepted"; else pass "a voice must be a number"; fi
: >"$LOG"; bash "$F" say "hello there"
contains "friday say speaks through friday-ctl" "$(cat "$LOG")" "friday-ctl-say hello there"
: >"$LOG"; bash "$F" voice speak setup kitten >/dev/null
contains "setup installs the chosen voice through friday-ctl" "$(cat "$LOG")" "friday-ctl-say --setup kitten"
: >"$LOG"; bash "$F" voice speak setup kokoro >/dev/null
contains "setup kokoro installs the expressive voice" "$(cat "$LOG")" "friday-ctl-say --setup kokoro"
: >"$LOG"; bash "$F" voice speak setup >/dev/null
contains "setup with no name installs tiny voca, the default voice" "$(cat "$LOG")" "friday-ctl-say --setup tinyvoca"
: >"$LOG"; bash "$F" voice speak voices >/dev/null
contains "voices asks friday-ctl for the installed voice's names" "$(cat "$LOG")" "friday-ctl-say --voices"
bash "$F" voice speak off >/dev/null
if [[ -e "$SPK" ]]; then fail "off left the switch on"; else pass "off removes the switch"; fi
: >"$LOG"; PATH="$TOUR/bin:$PATH" FRIDAY_SHELL_DIR="$TOUR/shell" bash "$F" tour
contains "friday tour starts the tour again, even after the first login" "$(cat "$LOG")" "ipc call tour begin"
# `friday app` opens FRIDAY Settings on a page through the shell.
: >"$LOG"; PATH="$TOUR/bin:$PATH" FRIDAY_SHELL_DIR="$TOUR/shell" bash "$F" app Voice
contains "friday app opens FRIDAY Settings on the page asked for" "$(cat "$LOG")" "ipc call settings open Voice"
# A shell that is slow to start: the tour keeps asking, then starts; it is not given up on or doubled with a notification.
SLOW="$W/slowtour"; mkdir -p "$SLOW/bin" "$SLOW/shell"; : >"$SLOW/shell/shell.qml"; rm -f "$SLOW/count"
printf '#!/bin/bash\nn=$(( $(cat "%s/count" 2>/dev/null || echo 0) + 1 )); echo $n >"%s/count"\necho "quickshell try $n $*" >>"%s"\n[ "$n" -ge 3 ]\n' "$SLOW" "$SLOW" "$LOG" >"$SLOW/bin/quickshell"; chmod +x "$SLOW/bin/quickshell"
: >"$LOG"; PATH="$SLOW/bin:$PATH" FRIDAY_SHELL_DIR="$SLOW/shell" FRIDAY_WELCOME_WAIT=5 FRIDAY_WELCOME_RETRY_TENTHS=1 bash "$F" welcome --force
contains "a slow shell: the tour keeps asking until it answers" "$(cat "$LOG")" "quickshell try 3"
if grep -q "^notify " "$LOG"; then fail "a slow shell also got the fallback notification"; else pass "no fallback once the tour started"; fi
# A shell that never comes up: the notification instead, and the welcome counts as shown.
rm -f "$SLOW/count" "${XDG_STATE_HOME:-$HOME/.local/state}/friday/welcomed"
printf '#!/bin/bash\necho "quickshell never" >>"%s"\nexit 1\n' "$LOG" >"$SLOW/bin/quickshell"
: >"$LOG"; PATH="$SLOW/bin:$PATH" FRIDAY_SHELL_DIR="$SLOW/shell" FRIDAY_WELCOME_WAIT=1 FRIDAY_WELCOME_RETRY_TENTHS=2 bash "$F" welcome
contains "no shell at all: the notification is shown" "$(cat "$LOG")" "Hello,"
: >"$LOG"; PATH="$SLOW/bin:$PATH" FRIDAY_SHELL_DIR="$SLOW/shell" FRIDAY_WELCOME_WAIT=1 bash "$F" welcome
if [[ ! -s "$LOG" ]]; then pass "and it is not shown again"; else fail "shown again after the fallback"; fi

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
echo "qs-start \$n backend=\${QT_QUICK_BACKEND:-gpu}" >> "$LOG"
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

# Software rendering: in VirtualBox from the first start; elsewhere after the
# second crash; never when asked for gpu.
vm() { printf '#!/bin/bash\n[ "$1" = --vm ] && echo %s\n' "$1" >"$S/systemd-detect-virt"; chmod +x "$S/systemd-detect-virt"; }
run_shell() { : >"$LOG"; rm -f "$SH/count" "$SH/qs.pid"; QS_CRASHES=${QS_CRASHES:-0} timeout 20 "${sh_env[@]}" "$@" bash "$F" shell start 2>/dev/null & }
vm oracle
: >"$LOG"; rm -f "$SH/count" "$SH/qs.pid"
"${sh_env[@]}" bash "$F" shell start 2>/dev/null &
SUP=$!; waitfor "started 1"
contains "in VirtualBox the shell starts in software rendering" "$(cat "$LOG")" "qs-start 1 backend=software"
"${sh_env[@]}" bash "$F" shell stop; waitfor "! kill -0 $SUP 2>/dev/null"
: >"$LOG"; rm -f "$SH/count" "$SH/qs.pid"
FRIDAY_SHELL_RENDER=gpu "${sh_env[@]}" bash "$F" shell start 2>/dev/null &
SUP=$!; waitfor "started 1"
contains "FRIDAY_SHELL_RENDER=gpu keeps the graphics card" "$(cat "$LOG")" "qs-start 1 backend=gpu"
"${sh_env[@]}" bash "$F" shell stop; waitfor "! kill -0 $SUP 2>/dev/null"
vm none
: >"$LOG"; rm -f "$SH/count" "$SH/qs.pid"
QS_CRASHES=2 "${sh_env[@]}" bash "$F" shell start 2>"$SH/err" &
SUP=$!; waitfor "started 3"
contains "on real hardware the first start uses the graphics card" "$(cat "$LOG")" "qs-start 1 backend=gpu"
contains "two crashes in a row switch to software" "$(cat "$LOG")" "qs-start 3 backend=software"
contains "and it says so" "$(cat "$SH/err")" "drawing it in software"
"${sh_env[@]}" bash "$F" shell stop; waitfor "! kill -0 $SUP 2>/dev/null"
rm -f "$S/systemd-detect-virt"

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
"${sh_env[@]}" bash "$F" shell palette classic >/dev/null
if grep -q active_border "$look"; then fail "the classic palette still overrides the desktop theme's border colours"
else pass "the classic palette leaves border colours to the desktop theme"; fi
# The palette's old name still works and is stored under the new one.
"${sh_env[@]}" bash "$F" shell palette frost >/dev/null
"${sh_env[@]}" bash "$F" shell palette omarchy >/dev/null
contains "the old palette name is saved as classic" "$(cat "$HOME/.config/friday/shell.json")" '"palette": "classic"'
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
# The inside-app engine doctor reports: agent-desktop when a 1.x copy is there.
fake_ad() { printf '#!/bin/bash\necho %q\n' '{"version":"2.6","ok":true,"command":"version","data":{"os":"linux","version":"'"$1"'"}}' >"$W/agent-desktop"; chmod +x "$W/agent-desktop"; }
fake_ad 1.3.1
out=$(FRIDAY_AGENT_DESKTOP="$W/agent-desktop" bash "$F" doctor 2>&1 || true)
contains "doctor names agent-desktop and its version" "$out" "inside-app control: agent-desktop 1.3.1"
fake_ad 2.0.0
out=$(FRIDAY_AGENT_DESKTOP="$W/agent-desktop" bash "$F" doctor 2>&1 || true)
contains "doctor warns about a version FRIDAY does not accept" "$out" "agent-desktop 2.0.0 is not a version FRIDAY accepts"
fake_ad 1.2.9
out=$(FRIDAY_AGENT_DESKTOP="$W/agent-desktop" bash "$F" doctor 2>&1 || true)
contains "doctor warns about too old a version" "$out" "agent-desktop 1.2.9 is not a version FRIDAY accepts"
out=$(FRIDAY_AGENT_DESKTOP="" PATH="$S:/usr/bin:/bin" bash "$F" doctor 2>&1 || true)
contains "without agent-desktop doctor says FRIDAY's own reader is used" "$out" "inside-app control: FRIDAY's own reader"
rm -f "$W/agent-desktop"
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
