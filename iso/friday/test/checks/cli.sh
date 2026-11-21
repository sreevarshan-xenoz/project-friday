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
if [[ "\$1" == watch ]]; then
  printf 'friday-ctl-watch %s\n' "\$*" >> "$LOG"
  exit 0
fi
if [[ "\$1" == recall ]]; then
  printf 'friday-ctl-recall %s\n' "\$*" >> "$LOG"
  [ -n "\$RECALL_REPLY" ] && printf '%s\n' "\$RECALL_REPLY"
  exit 0
fi
if [[ "\$1" == docs ]]; then
  printf 'friday-ctl-docs %s\n' "\$*" >> "$LOG"
  [ -n "\$DOCS_REPLY" ] && printf '%s\n' "\$DOCS_REPLY"
  exit 0
  exit 0
fi
if [[ "\$1" == show ]]; then
  printf 'friday-ctl-show %s\n' "\$*" >> "$LOG"
  if [[ "\$2" == --exact ]]; then
    [[ "\$3" == Appearance ]] && { echo '{"found":true,"name":"Appearance","x":30,"y":200,"width":180,"height":40}'; exit 0; }
    echo '{"found":false,"app":"Settings","controls":[]}'; exit 0
  fi
  none='{"found":false,"error":"not on the bus"}'
  printf '%s\n' "\${SHOW_REPLY:-\$none}"
  exit 0
fi
if [[ "\$*" == *"should they click first"* ]]; then
  printf 'friday-ctl-pick %s\n' "\$*" >> "$LOG"
  echo '{"accepted":true}'; printf '{\n "outcome": "completed",\n "result": "Appearance"\n}\n'; exit 0
fi
if [[ "\$1" == routine ]]; then
  printf 'friday-ctl-routine %s\n' "\$*" >> "$LOG"
  echo "stub routine output"
  exit 0
fi
if [[ "\$1" == thought ]]; then
  printf 'friday-ctl-thought %s\n' "\$*" >> "$LOG"
  echo "Kept that \$3."
  exit 0
fi
if [[ "\$1" == workspace && "\$2" == project ]]; then
  # A local lookup (the project in front), not a question for the daemon.
  printf 'ctl-project\n' >> "$LOG"
  [ -n "\${WS_PROJECT:-}" ] && echo "\$WS_PROJECT"
  exit 0
fi
if [[ "\$1" == workspace ]]; then
  printf 'friday-ctl-workspace %s\n' "\$*" >> "$LOG"
  [[ "\$2" == list ]] && [ -n "\${WS_SAVED:-}" ] && echo "friday  3 window(s)  saved 2026-11-21 09:00"
  if [[ "\$2" == restore ]]; then
    [ -n "\${WS_SAVED:-}" ] || exit 1
    echo 'Workspace "friday": opening foot in friday (nvim).'
  fi
  exit 0
fi
if [[ "\$1" == listen ]]; then
  printf 'friday-ctl-listen %s\n' "\$*" >> "$LOG"
  echo "/notes/2026-11-20-0905.txt"
  sleep 30
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
if [[ "\$target_arg" == *"rewritten text"* && "\$target_arg" == *teh* ]]; then printf '{\n "outcome": "completed",\n "result": "\\\\"The quick fox.\\\\""\n}\n'; exit 0; fi
if [[ "\$target_arg" == *useditems* ]]; then printf '{\n "outcome": "completed",\n "result": "Done.",\n "used": ["your timeline", "a lesson from a failure"]\n}\n'; exit 0; fi
if [[ "\$target_arg" == *"word: ready"* && -n "\${READY_FAILS:-}" ]]; then printf '{\n "outcome": "failed",\n "error": "All candidates failed (2 tried). opencode: 429"\n}\n'; exit 1; fi
if [[ "\$target_arg" == *"word: ready"* && -n "\${READY_TIMES_OUT:-}" ]]; then printf '{"accepted":true}\n'; echo "timed out after 60s waiting for the goal to finish" >&2; exit 1; fi
if [[ "\$target_arg" == *"word: ready"* ]]; then printf '{\n "outcome": "completed",\n "result": "ready",\n "served_by": {"provider": "kilo", "model": "m", "ms": 940}\n}\n'; exit 0; fi
if [[ "\$target_arg" == *"meeting transcript"* ]]; then printf '{\n "outcome": "completed",\n "result": "decision|10:02:15|Ship v0.8 on Friday\\\\naction|10:04:01|Sam writes the release notes\\\\nquestion|10:05:30|Do we need a beta?\\\\nsome chatter"\n}\n'; exit 0; fi
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

echo "the ask box opens first"
# The project lookup and FRIDAY Shell's pill can take seconds; the box must
# not wait for them.
mkdir -p "$W/slow-bin"
printf '#!/bin/bash\n[ "$1 $2" = "workspace project" ] && { sleep 2; echo /home/me/code/x; exit 0; }\nexec %q "$@"\n' "$S/friday-ctl" >"$W/slow-bin/friday-ctl"
printf '#!/bin/bash\ndate +%%s%%N >%q\nexit 1\n' "$W/box-at" >"$W/slow-bin/fuzzel"
chmod +x "$W/slow-bin"/*
rm -f "$W/box-at"; t0=$(date +%s%N); PATH="$W/slow-bin:$PATH" bash "$F" ask >/dev/null 2>&1 || true
if [ -s "$W/box-at" ]; then
  box_ms=$(( ( $(cat "$W/box-at") - t0 ) / 1000000 ))
  (( box_ms < 1500 )) && pass "the ask box opens in ${box_ms} ms with slow lookups behind it" || fail "the ask box took ${box_ms} ms (lookups ran first)"
else fail "the ask box never opened"; fi
sleep 2.5  # the slow lookups end before the next checks read the log

echo "password-shaped text"
shaped=$(bash -c 'source <(sed -n "/^friday_secret_shaped() {/,/^}/p" "$1"); shift; for t in "$@"; do friday_secret_shaped "$t" && printf "S" || printf "."; done' _ "$F" \
  "https://github.com/x/y/issues/1234" "/home/me/notes/file2.txt" 'C:\Users\me\file2.txt' "report2026.pdf" "hello world 123" \
  "tok_abc123def456ghi789jkl012" "abcdefghij1234567890.klmnopqrst0987654321.uvwxyzABC123def456" \
  "https://api.x.com/v1?key=zz_abcd1234efgh5678ijkl" "hunter2hunter2" \
  "https://x.com/cb?id=ab12ab12ab12ab12" "Zq8rTk2LmN0p/Wx4Yb7Vc1Ds/Ee9Ff3Gh5Jk6" "hunter2hunter2." "pass/12345678abc")
[[ "$shaped" == ".....SSSSSSSS" ]] && pass "URLs, paths and file names go; tokens, JWTs, keys in URLs and base64 don't" || fail "password-shaped rule: got $shaped, wanted .....SSSSSSSS"

echo "instant answers"
printf '#!/bin/bash\necho "qalc $*" >>%q\ncase "$3" in "12 * 7") echo 84 ;; "15/100*240") echo 36 ;; "100 °F to °C") echo "37.77777778 °C" ;; *) echo "?" ;; esac\n' "$LOG" >"$S/qalc"; chmod +x "$S/qalc"
: >"$LOG"; bash "$F" ask "what is 12 x 7"; sleep 0.3
contains "arithmetic is worked out here" "$(cat "$state/last-answer.md")" "12 * 7 = 84"
if grep -q "^friday-ctl" "$LOG"; then fail "arithmetic went to the model"; else pass "arithmetic goes to no model"; fi
: >"$LOG"; bash "$F" ask "15% of 240"; sleep 0.3
contains "a percentage too" "$(cat "$state/last-answer.md")" "= 36"
: >"$LOG"; bash "$F" ask "100 f to c"; sleep 0.3
contains "and a unit conversion, with f and c as degrees" "$(cat "$state/last-answer.md")" "100 °F to °C = 37.77777778 °C"
: >"$LOG"; bash "$F" ask "what is a monad"; sleep 0.3
if grep -q "^qalc" "$LOG"; then fail "a question in words went to qalc"; else pass "a question in words goes to the model, not qalc"; fi
rm -f "$S/qalc"
shapes=$(bash -c 'source <(sed -n "/^friday_calc_expr() {/,/^}/p" "$1"); shift; for q in "$@"; do friday_calc_expr "$q" >/dev/null && printf "Y" || printf "."; done' _ "$F" \
  "2026-11-22" "123-456-7890" "192.0.2.1/24" "1.0.0-1" "24/7" "50/50" "--5 + 2" "10 - 3" "-40 c to f")
[[ "$shapes" == "......YYY" ]] && pass "dates, phone numbers, IPs, versions and 24/7 aren't sums" || fail "calc shapes: got $shapes, wanted ......YYY"

echo "suggestions for the app you're in"
printf '#!/bin/bash\necho "hyprctl $*" >> %q\n[[ "$*" == "activewindow -j" ]] && printf %%s "$HYPR_WIN"\nexit 0\n' "$LOG" >"$S/hyprctl"; chmod +x "$S/hyprctl"
printf '#!/bin/bash\ncat >%q\necho "$PICK"\n' "$W/menu.txt" >"$S/fuzzel"
: >"$LOG"; HYPR_WIN='{"class":"firefox"}' PICK="Summarize this page" bash "$F" ask; sleep 0.3
contains "a browser offers to summarize the page" "$(head -n1 "$W/menu.txt")" "Summarize this page"
contains "picking it asks about the screen" "$(cat "$LOG")" "friday-ctl-context"
contains "with that question" "$(cat "$LOG")" "Summarize this page"
: >"$LOG"; HYPR_WIN='{"class":"code"}' PRIMARY="fn main() { let x: i32 = \"a\"; }" PICK="Explain the selected code" bash "$F" ask; sleep 0.3
contains "an editor with a selection offers to explain the code" "$(cat "$W/menu.txt")" "Explain the selected code"
contains "picking it sends the selection" "$(cat "$LOG")" "let x: i32"
: >"$LOG"; HYPR_WIN='{"class":"org.gnome.Calculator"}' PICK="what is rust" bash "$F" ask; sleep 0.3
if grep -qE "Summarize|Explain the selected" "$W/menu.txt"; then fail "suggested something for an app with nothing to offer"; else pass "an app with no page and no selection: no suggestions"; fi
printf '#!/bin/bash\necho "fuzzel $*" >> %q\necho "$PICK"\n' "$LOG" >"$S/fuzzel"; chmod +x "$S/fuzzel"
rm -f "$S/hyprctl"

echo "explain the last error (kitty)"
# A focused kitty window (pid 4242) whose last command failed; kitty's own
# remote control answers ls and get-text (stubbed), on its real socket path.
RUN="$W/run"; mkdir -p "$RUN"
python3 -c 'import socket,sys; socket.socket(socket.AF_UNIX).bind(sys.argv[1])' "$RUN/omarchy-kitty-4242"
printf '#!/bin/bash\necho "hyprctl $*" >> %q\n[[ "$*" == "activewindow -j" ]] && printf %%s "$HYPR_WIN"\nexit 0\n' "$LOG" >"$S/hyprctl"
cat >"$S/kitten" <<STUB
#!/bin/bash
echo "kitten \$*" >>"$LOG"
case "\$*" in
  *" ls"*) printf '%s' "\$KITTY_LS" ;;
  *get-text*) printf 'npm ERR! Missing script: "build"\nnpm ERR! A complete log of this run can be found in /tmp/x.log\n' ;;
esac
STUB
chmod +x "$S/hyprctl" "$S/kitten"
kls() { printf '[{"is_focused":true,"tabs":[{"is_focused":true,"windows":[{"id":7,"is_focused":true,"at_prompt":true,"cwd":"/home/me/app","last_reported_cmdline":"npm run build","last_cmd_exit_status":%s}]}]}]' "$1"; }
export HYPR_WIN='{"class":"kitty","pid":4242,"title":"zsh"}'
: >"$LOG"; KITTY_LS=$(kls 1) XDG_RUNTIME_DIR="$RUN" PICK="Explain the error from: npm run build (exit 1)" bash "$F" ask; sleep 0.3
contains "Super+Y in kitty offers to explain the failed command" "$(cat "$LOG")" "fuzzel"
contains "it reads the output from kitty" "$(cat "$LOG")" "get-text --match id:7 --extent last_cmd_output"
contains "the model gets the command" "$(cat "$LOG")" "npm run build"
contains "and its output" "$(cat "$LOG")" 'Missing script'
contains "and asks how to fix it" "$(cat "$LOG")" "Explain why this command failed"
if grep -q "Explain why this command failed" "$HOME/.local/state/friday/ask-history" 2>/dev/null; then fail "an explained error went into ask history"
else pass "an explained error stays out of ask history"; fi
: >"$LOG"; KITTY_LS=$(kls 0) XDG_RUNTIME_DIR="$RUN" PICK="what is rust" bash "$F" ask; sleep 0.3
if grep -q "get-text" "$LOG"; then fail "read output after a command that worked"; else pass "nothing to explain after a command that worked"; fi
: >"$LOG"; KITTY_LS=$(kls 130) XDG_RUNTIME_DIR="$RUN" PICK="what is rust" bash "$F" ask; sleep 0.3
if grep -q "get-text" "$LOG"; then fail "offered to explain a Ctrl+C"; else pass "a Ctrl+C (exit 130) isn't an error"; fi
# A command still running there can be watched instead.
: >"$LOG"; KITTY_LS='[{"is_focused":true,"tabs":[{"is_focused":true,"windows":[{"id":7,"is_focused":true,"at_prompt":false,"last_reported_cmdline":"make build","last_cmd_exit_status":0}]}]}]' \
  XDG_RUNTIME_DIR="$RUN" PICK="Tell me when this finishes: make build" bash "$F" ask; sleep 0.5
contains "a running command can be watched" "$(cat "$LOG")" "friday-ctl-watch watch kitty $RUN/omarchy-kitty-4242 7 make build"
contains "and it says so" "$(cat "$LOG")" "I'll tell you when make build finishes"
if grep -q "^friday-ctl '" "$LOG"; then fail "watching a command asked the AI"; else pass "watching a command asks no AI"; fi
export HYPR_WIN='{"class":"firefox","pid":4242,"title":"x"}'
: >"$LOG"; KITTY_LS=$(kls 1) XDG_RUNTIME_DIR="$RUN" PICK="what is rust" bash "$F" ask; sleep 0.3
if grep -q "^kitten" "$LOG"; then fail "asked kitty while another app had focus"; else pass "only a focused kitty window is asked"; fi
unset HYPR_WIN
rm -f "$S/kitten"

echo "explain the last error (any terminal, friday shell-hook)"
contains "the zsh hook records each command" "$(bash "$F" shell-hook zsh)" "add-zsh-hook preexec _friday_preexec"
contains "the bash hook too" "$(bash "$F" shell-hook bash)" 'PROMPT_COMMAND="_friday_prompt'
# A foot window (its pid) with a shell under it that left a record.
# script gives the shell a terminal of its own (WezTerm's pane is found by it).
script -qc "bash -c 'sleep 30; :'" /dev/null </dev/null >/dev/null 2>&1 & TERM_WIN=$!
for _ in 1 2 3 4 5 6 7 8 9 10; do TERM_SH=$(pgrep -P "$TERM_WIN" | head -n1); [ -n "$TERM_SH" ] && break; sleep 0.1; done
mkdir -p "$RUN/friday-term"
printf 'done\t%s\t2\t/home/me/app\tmake test\n' "$(date +%s)" >"$RUN/friday-term/$TERM_SH"
export HYPR_WIN="{\"class\":\"foot\",\"pid\":$TERM_WIN,\"title\":\"zsh\"}"
printf '#!/bin/bash\ncat >%q\necho "$PICK"\n' "$W/menu.txt" >"$S/fuzzel"
: >"$LOG"; XDG_RUNTIME_DIR="$RUN" PICK="what is rust" bash "$F" ask; sleep 0.3
contains "Super+Y in foot offers to explain the failed command" "$(head -n1 "$W/menu.txt")" "Explain the error from: make test (exit 2)"
: >"$LOG"; XDG_RUNTIME_DIR="$RUN" PICK="Explain the error from: make test (exit 2)" bash "$F" ask; sleep 0.3
contains "with no way to read its output, FRIDAY looks at the window" "$(cat "$LOG")" "context screen --yes --ask"
contains "and the model gets the command and exit code" "$(cat "$LOG")" "Command: make test. Folder: /home/me/app. Exit code: 2."
# WezTerm can give the pane's text: that goes instead of a screenshot.
cat >"$S/wezterm" <<'STUB'
#!/bin/bash
case "$*" in
  "cli list --format json") printf '[{"pane_id":2,"tty_name":"/dev/pts/999"},{"pane_id":3,"tty_name":"%s"}]' "$PANE_TTY" ;;
  "cli get-text --pane-id 3 --start-line -150") printf 'make: *** No rule to make target test.  Stop.\n' ;;
esac
STUB
chmod +x "$S/wezterm"
export PANE_TTY="/dev/$(ps -o tty= -p "$TERM_SH" | tr -d ' ')"
export HYPR_WIN="{\"class\":\"org.wezfurlong.wezterm\",\"pid\":$TERM_WIN,\"title\":\"zsh\"}"
: >"$LOG"; XDG_RUNTIME_DIR="$RUN" PICK="Explain the error from: make test (exit 2)" bash "$F" ask; sleep 0.3
contains "in WezTerm the model gets the pane's text" "$(cat "$LOG")" "No rule to make target test"
if grep -q "context screen" "$LOG"; then fail "took a screenshot in WezTerm"; else pass "WezTerm: no screenshot"; fi
rm -f "$S/wezterm"
export HYPR_WIN="{\"class\":\"foot\",\"pid\":$TERM_WIN,\"title\":\"zsh\"}"
printf 'done\t%s\t2\t/home/me/app\tmake test\n' "$(( $(date +%s) - 3600 ))" >"$RUN/friday-term/$TERM_SH"
: >"$LOG"; XDG_RUNTIME_DIR="$RUN" PICK="what is rust" bash "$F" ask; sleep 0.3
if grep -q "Explain the error" "$W/menu.txt"; then fail "offered an hour-old failure"; else pass "an old failure isn't offered"; fi
printf 'running\t%s\t-\t/home/me/app\tcargo build\n' "$(date +%s)" >"$RUN/friday-term/$TERM_SH"
: >"$LOG"; XDG_RUNTIME_DIR="$RUN" PICK="Tell me when this finishes: cargo build" bash "$F" ask; sleep 0.3
contains "a running command can be watched" "$(cat "$LOG")" "I'll tell you when cargo build finishes"
printf '#!/bin/bash\necho "fuzzel $*" >> %q\necho "$PICK"\n' "$LOG" >"$S/fuzzel"; chmod +x "$S/fuzzel"
: >"$LOG"; XDG_RUNTIME_DIR="$RUN" timeout 10 bash "$F" term-watch "$TERM_SH" & WATCH=$!
sleep 0.5; printf 'done\t%s\t101\t/home/me/app\tcargo build\n' "$(date +%s)" >"$RUN/friday-term/$TERM_SH"
wait "$WATCH" 2>/dev/null || true
contains "and it says how it ended" "$(cat "$LOG")" "Failed (exit 101): cargo build"
kill "$TERM_WIN" 2>/dev/null || true
unset HYPR_WIN
rm -rf "$S/hyprctl" "$RUN/friday-term"

echo "rewrite in place"
# hyprctl: the focused window (tags from $WIN_TAGS). With $MOVE_AFTER=N, the
# Nth look and later see another window (the person switched windows).
cat >"$S/hyprctl" <<STUB
#!/bin/bash
echo "hyprctl \$*" >>"$LOG"
if [[ "\$*" == "activewindow -j" ]]; then
  n=\$(( \$(cat "$W/looks" 2>/dev/null || echo 0) + 1 )); echo \$n >"$W/looks"
  addr=0xaaa; [ -n "\${MOVE_AFTER:-}" ] && (( n >= MOVE_AFTER )) && addr=0xbbb
  printf '{"class":"org.gnome.TextEditor","address":"%s","tags":[%s]}' "\$addr" "\${WIN_TAGS:-}"
fi
exit 0
STUB
chmod +x "$S/hyprctl"; rm -f "$W/looks"
: >"$LOG"; PRIMARY="teh quick fox" PICK="Rewrite: fix grammar and spelling" bash "$F" ask --selection; sleep 0.5
contains "asks for only the rewritten text" "$(cat "$LOG")" "Reply with only the rewritten text"
contains "the rewrite goes on the clipboard, quotes stripped" "$(cat "$LOG")" "wl-copy: The quick fox."
contains "and is pasted over the selection (Ctrl+V down)" "$(cat "$LOG")" 'send_key_state({ mods = "CTRL", key = "V", state = "down" })'
contains "and up again" "$(cat "$LOG")" 'state = "up"'
if grep -q "Rewrite: fix grammar" "$HOME/.local/state/friday/ask-history" 2>/dev/null; then fail "a rewrite went into ask history"; else pass "a rewrite stays out of ask history"; fi
# The person moved to another window while the model answered: no paste.
# (Looks 1-3 are taken with the selection and the private check; the 4th is when the answer is back.)
: >"$LOG"; rm -f "$W/looks"; MOVE_AFTER=4 PRIMARY="teh quick fox" PICK="Rewrite: fix grammar and spelling" bash "$F" ask --selection; sleep 0.5
if grep -q "send_key_state" "$LOG"; then fail "pasted into a window the person moved to"; else pass "another window by then: no paste"; fi
contains "it says the rewrite is on the clipboard" "$(cat "$LOG")" "on your clipboard"
rm -f "$W/looks"
# Undo after a rewrite: Ctrl+Z in that window; another window in front: the
# original goes on the clipboard.
: >"$LOG"; PRIMARY="teh quick fox" PICK="Rewrite: fix grammar and spelling" bash "$F" ask --selection; sleep 0.5
rm -f "$W/looks"; : >"$LOG"; out=$(bash "$F" undo)
contains "undo after a rewrite: Ctrl+Z in that window" "$(cat "$LOG")" 'send_key_state({ mods = "CTRL", key = "Z", state = "down" })'
contains "and says what it undid" "$out" "Undid the rewrite"
: >"$LOG"; PRIMARY="teh quick fox" PICK="Rewrite: fix grammar and spelling" bash "$F" ask --selection; sleep 0.5
rm -f "$W/looks"; : >"$LOG"; out=$(MOVE_AFTER=1 bash "$F" undo)
if grep -q 'key = "Z"' "$LOG"; then fail "sent Ctrl+Z to another window"; else pass "another window in front: no Ctrl+Z"; fi
contains "the original text goes on the clipboard" "$(cat "$LOG")" "wl-copy: teh quick fox"
contains "and it says how to undo" "$out" "press Ctrl+Z"
# The same window, but a terminal now has it (an address reused): no Ctrl+Z.
: >"$LOG"; PRIMARY="teh quick fox" PICK="Rewrite: fix grammar and spelling" bash "$F" ask --selection; sleep 0.5
rm -f "$W/looks"; : >"$LOG"; WIN_TAGS='"terminal*"' bash "$F" undo >/dev/null
if grep -q 'key = "Z"' "$LOG"; then fail "sent Ctrl+Z to a terminal"; else pass "a terminal in front: no Ctrl+Z"; fi
rm -f "$W/looks"
: >"$LOG"; WIN_TAGS='"terminal*"' PRIMARY="teh quick fox" PICK="Rewrite: fix grammar and spelling" bash "$F" ask --selection; sleep 0.5
if grep -q "send_key_state" "$LOG"; then fail "pasted into a terminal"; else pass "a terminal gets the rewrite on the clipboard, not pasted"; fi
# Text that only came from the clipboard can't be replaced: no rewrite entries.
printf '#!/bin/bash\ncat >>%q\necho "$PICK"\n' "$W/menu.txt" >"$S/fuzzel"; : >"$W/menu.txt"
CLIP="copied words here" PICK="Summarize this" bash "$F" ask --selection; sleep 0.3
if grep -q "^Rewrite:" "$W/menu.txt"; then fail "offered to rewrite text that wasn't selected"; else pass "clipboard text: no rewrite entries"; fi
: >"$W/menu.txt"; PRIMARY="teh quick fox" PICK="Summarize this" bash "$F" ask --selection; sleep 0.3
contains "highlighted text: rewrite entries first" "$(head -n1 "$W/menu.txt")" "Rewrite: fix grammar and spelling"
printf '#!/bin/bash\necho "fuzzel $*" >> %q\necho "$PICK"\n' "$LOG" >"$S/fuzzel"; chmod +x "$S/fuzzel"

echo "paste as"
rm -f "$W/looks"
: >"$LOG"; CLIP="teh notes" PICK="As a bullet list" bash "$F" paste-as; sleep 0.3
contains "paste-as transforms the clipboard" "$(cat "$LOG")" "Rewrite the text below As a bullet list"
contains "and pastes the result" "$(cat "$LOG")" 'send_key_state({ mods = "CTRL", key = "V", state = "down" })'
: >"$LOG"; WIN_TAGS='"terminal*"' CLIP="teh notes" PICK="Shorter" bash "$F" paste-as; sleep 0.3
contains "in a terminal it pastes with Shift+Insert" "$(cat "$LOG")" 'mods = "SHIFT", key = "Insert"'
: >"$LOG"; CLIP="hunter2Secret99xyz" PICK="Shorter" bash "$F" paste-as; sleep 0.3
if grep -q "^friday-ctl" "$LOG"; then fail "paste-as sent a password-shaped clipboard"; else pass "paste-as never sends a password-shaped clipboard"; fi
: >"$LOG"; CLIP="" PICK="Shorter" bash "$F" paste-as; sleep 0.3
contains "an empty clipboard says what to do" "$(cat "$LOG")" "Copy some text first"

echo "clips and snippets"
CLIPS="$state/clips.jsonl"
rm -f "$CLIPS" "$W/looks"
printf 'first note' | bash "$F" clips add
printf 'second note\nwith two lines' | bash "$F" clips add
printf 'first note' | bash "$F" clips add
expect "a copied text is kept" test "$(wc -l <"$CLIPS")" -eq 2
contains "copying it again moves it to the top" "$(tail -n1 "$CLIPS")" '"text":"first note"'
contains "clips are owner-only" "$(stat -c %a "$CLIPS")" "600"
printf 'hunter2Secret99xyz' | bash "$F" clips add
if grep -q hunter2 "$CLIPS"; then fail "kept a password-shaped clip"; else pass "a password-shaped clip isn't kept"; fi
printf 'vault words' | CLIPBOARD_STATE=sensitive bash "$F" clips add
if grep -q "vault words" "$CLIPS"; then fail "kept a clip marked sensitive"; else pass "a clip marked sensitive isn't kept"; fi
printf '   ' | bash "$F" clips add
expect "blank text isn't kept" test "$(wc -l <"$CLIPS")" -eq 2
for i in $(seq 1 205); do printf '{"ts":1,"text":"old %s"}\n' "$i"; done >"$CLIPS"
printf 'newest' | bash "$F" clips add
expect "only the last 200 are kept" test "$(wc -l <"$CLIPS")" -eq 200
printf '{"ts":1,"text":"first note"}\n{"ts":2,"text":"second note\\nwith two lines"}\n' >"$CLIPS"
bash "$F" snippet add sig "Best, Sam" >/dev/null
contains "snippets are listed by name" "$(bash "$F" snippet list)" ";sig  Best, Sam"
printf '#!/bin/bash\ncat >%q\necho "$PICK"\n' "$W/menu.txt" >"$S/fuzzel"
: >"$LOG"; PICK=";sig  Best, Sam" bash "$F" clips pick; sleep 0.3
contains "the picker lists snippets" "$(cat "$W/menu.txt")" ";sig  Best, Sam"
contains "and clips newest first, on one line" "$(sed -n 2p "$W/menu.txt")" "1  second note with two lines"
contains "a picked snippet is pasted as it is" "$(cat "$LOG")" "wl-copy: Best, Sam"
: >"$LOG"; PICK="2  first note" bash "$F" clips pick; sleep 0.3
contains "a picked clip offers As it is first" "$(head -n1 "$W/menu.txt")" "As it is"
printf '#!/bin/bash\necho "fuzzel $*" >> %q\nif [[ "$*" == *Clips* ]]; then echo "2  first note"; else echo "Shorter"; fi\n' "$LOG" >"$S/fuzzel"
: >"$LOG"; bash "$F" clips pick; sleep 0.3
contains "a clip can be rewritten (paste as)" "$(cat "$LOG")" "Rewrite the text below Shorter"
contains "the clip goes to the model, not the clipboard" "$(grep -A2 'Rewrite the text below' "$LOG")" "first note"
printf '#!/bin/bash\necho "fuzzel $*" >> %q\necho "$PICK"\n' "$LOG" >"$S/fuzzel"; chmod +x "$S/fuzzel"
bash "$F" snippet rm sig >/dev/null
contains "the snippet file stays owner-only" "$(stat -c %a "$HOME/.config/friday/snippets.jsonl")" "600"
if bash "$F" snippet list | grep -q ";sig"; then fail "snippet rm left it"; else pass "snippet rm removes it"; fi
refuse "a snippet name with spaces is refused" bash "$F" snippet add "two words" x
: >"$LOG"; bash "$F" clips off >/dev/null
expect "clips off forgets them all" test ! -e "$CLIPS"
contains "and stops the service" "$(cat "$LOG")" "systemctl --user disable --now friday-clips.service"
rm -f "$S/hyprctl"
# A private window in front: nothing is kept.
printf '#!/bin/bash\n[[ "$*" == "activewindow -j" ]] && printf %%s %q\nexit 0\n' '{"class":"org.keepassxc.KeePassXC","title":"vault"}' >"$S/hyprctl"; chmod +x "$S/hyprctl"
printf 'copied in the vault' | bash "$F" clips add
expect "nothing is kept from a private window" test ! -s "$CLIPS"
rm -f "$S/hyprctl"

echo "private windows"
# The focused window is $HYPR_WIN; curl answers as the local model ($LOCAL_REPLY).
printf '#!/bin/bash\n[[ "$*" == "activewindow -j" ]] && printf %%s "$HYPR_WIN"\nexit 0\n' >"$S/hyprctl"
cp "$S/curl" "$W/curl.saved"
cat >"$S/curl" <<STUB
#!/bin/bash
echo "curl \$*" >>"$LOG"
[ -n "\${LOCAL_REPLY:-}" ] && jq -nc --arg r "\$LOCAL_REPLY" '{choices: [{message: {content: \$r}}]}'
exit 0
STUB
chmod +x "$S/hyprctl" "$S/curl"
envf="$HOME/.config/friday/friday.env"; mkdir -p "$HOME/.config/friday"; cp "$envf" "$W/env.saved" 2>/dev/null || : >"$W/env.saved"
export HYPR_WIN='{"class":"org.keepassxc.KeePassXC","title":"Passwords.kdbx"}'
: >"$LOG"; : >"$envf"; PRIMARY="hunter2 secret note" PICK="explain" bash "$F" ask --selection; sleep 0.3
if grep -q "^friday-ctl" "$LOG"; then fail "a password manager's text went to the daemon"; else pass "a password manager's text never reaches the daemon"; fi
contains "no local model: it says why and how" "$(cat "$LOG")" "friday setup-ai local"
printf 'LLAMACPP_ENABLED=1\nLLAMACPP_BASE_URL=http://127.0.0.1:8080/v1\nLLAMACPP_MODEL=qwen3:4b\n' >"$envf"
: >"$LOG"; LOCAL_REPLY="It is a note." PRIMARY="hunter2 secret note" PICK="explain" bash "$F" ask --selection; sleep 0.3
contains "with a local model it asks it directly" "$(cat "$LOG")" "http://127.0.0.1:8080/v1/chat/completions"
contains "and shows its answer" "$(cat "$LOG")" "It is a note."
contains "the saved answer is marked as kept here (the timeline skips it)" "$(cat "$HOME/.local/state/friday/last-answer.md" 2>/dev/null)" "_On this computer · nothing was sent_"
if grep -q "^friday-ctl" "$LOG"; then fail "the private ask went through the daemon"; else pass "the private ask skips the daemon"; fi
if grep -q "hunter2" "$HOME/.local/state/friday/last-answer.md"; then fail "the saved answer kept the selected text"
else pass "the saved answer says there was text, not what it was"; fi
: >"$LOG"; LOCAL_REPLY="An example." HYPR_WIN='{"class":"kitty","title":"zsh"}' bash "$F" ask --follow-up "give an example"; sleep 0.3
contains "a follow-up to a local answer stays local" "$(cat "$LOG")" "http://127.0.0.1:8080/v1/chat/completions"
if grep -q "^friday-ctl" "$LOG"; then fail "a local answer's follow-up went to the daemon"; else pass "a local answer's follow-up skips the daemon"; fi
# A local model that isn't on this computer doesn't count.
printf 'LLAMACPP_ENABLED=1\nLLAMACPP_BASE_URL=http://192.0.2.7:8080/v1\n' >"$envf"
: >"$LOG"; LOCAL_REPLY="x" PRIMARY="hunter2" PICK="explain" bash "$F" ask --selection; sleep 0.3
if grep -q "192.0.2.7" "$LOG"; then fail "sent private text to another machine"; else pass "a model on another machine isn't local"; fi
printf 'LLAMACPP_ENABLED=1\nLLAMACPP_BASE_URL=http://127.0.0.1:8080/v1\n' >"$envf"
: >"$LOG"; bash "$F" ask --screen "what is this"; sleep 0.3
if grep -q "context screen" "$LOG"; then fail "took a screenshot of a private window"; else pass "no screenshot of a private window"; fi
contains "and says so" "$(cat "$LOG")" "no screenshot of it leaves"
# "never <name>": not even the local model reads that window.
cp "$HOME/.config/friday/private-windows" "$W/pw.saved" 2>/dev/null || : >"$W/pw.saved"
printf 'never thunderbird\n~/work/clients\n' >"$HOME/.config/friday/private-windows"
export HYPR_WIN='{"class":"thunderbird","title":"Inbox"}'
: >"$LOG"; LOCAL_REPLY="x" PRIMARY="the contract terms" PICK="explain" bash "$F" ask --selection; sleep 0.3
if grep -q "chat/completions\|^friday-ctl" "$LOG"; then fail "a never window was read"; else pass "a never window isn't read, not even locally"; fi
contains "and it says which rule" "$(cat "$LOG")" 'your rule "never thunderbird"'
contains "a folder line marks folders private" "$(bash -c 'source <(sed -n "/^friday_private_folder()/,/^}/p" "$1"); friday_private_folder "$HOME/work/clients/acme"' _ "$F")" "~/work/clients"
if [ -n "$(bash -c 'source <(sed -n "/^friday_private_folder()/,/^}/p" "$1"); friday_private_folder "$HOME/work/clientside"' _ "$F")" ]; then fail "a folder rule matched a sibling folder"; else pass "a folder rule matches only inside it"; fi
printf 'never ~/vault\n' >"$HOME/.config/friday/private-windows"
contains "a never folder stays never" "$(bash -c 'source <(sed -n "/^friday_private_folder()/,/^}/p" "$1"); friday_private_folder "$HOME/vault/"' _ "$F")" "never:~/vault"
export HYPR_WIN='{"class":"foot","title":"~/vault notes"}'
: >"$LOG"; PRIMARY="x" PICK="explain" bash "$F" ask --selection; sleep 0.3
if grep -q "your rule" "$LOG"; then fail "a folder line was matched against a window title"; else pass "folder lines aren't matched against windows"; fi
cp "$W/pw.saved" "$HOME/.config/friday/private-windows"
export HYPR_WIN='{"class":"firefox","title":"Bank statement — Mozilla Firefox Private Browsing"}'
: >"$LOG"; LOCAL_REPLY="ok" PRIMARY="balance 12" PICK="explain" bash "$F" ask --selection; sleep 0.3
contains "private browsing counts" "$(cat "$LOG")" "127.0.0.1:8080"
# Your own list adds windows; "!word" drops a built-in one.
export HYPR_WIN='{"class":"org.gnome.TextEditor","title":"diary.txt"}'
printf 'Diary\n!bank\n' >"$HOME/.config/friday/private-windows"
: >"$LOG"; LOCAL_REPLY="ok" PRIMARY="dear diary" PICK="explain" bash "$F" ask --selection; sleep 0.3
contains "your own list counts" "$(cat "$LOG")" "127.0.0.1:8080"
export HYPR_WIN='{"class":"firefox","title":"My bank"}'
: >"$LOG"; PRIMARY="rates" PICK="explain" bash "$F" ask --selection; sleep 0.3
contains "a dropped built-in goes to the daemon as usual" "$(cat "$LOG")" "friday-ctl"
export HYPR_WIN='{"class":"firefox","title":"Rust docs"}'
: >"$LOG"; PRIMARY="fn main" PICK="explain" bash "$F" ask --selection; sleep 0.3
if grep -q "127.0.0.1:8080" "$LOG"; then fail "an ordinary window was treated as private"; else pass "an ordinary window asks as usual"; fi
unset HYPR_WIN
rm -f "$HOME/.config/friday/private-windows" "$S/hyprctl"; cp "$W/env.saved" "$envf"; mv "$W/curl.saved" "$S/curl"
# `friday forget` shows counts, asks before deleting, and wipes only what was named.
FD="$HOME/.local/state/friday"; mkdir -p "$FD/memory"
printf '<!-- friday answer -->\n# q1\n\nA\n\n---\n\n<!-- friday answer -->\n# q2\n\nB\n\n---\n\n' >"$FD/answers.md"
printf 'q2\nq1\n' >"$FD/ask-history"; printf '{"a":1}\n{"a":2}\n{"a":3}\n' >"$FD/egress.jsonl"
printf '{"e":1}\n' >"$FD/memory/episodes.jsonl"; printf '{"ts":1}\n' >"$FD/lessons.jsonl"
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
[[ ! -e "$FD/egress.jsonl" && ! -e "$FD/memory/episodes.jsonl" && ! -e "$FD/lessons.jsonl" ]] && pass "forget all deletes the rest, lessons too" || fail "forget all left files"
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
contains "screen: prompt says what is shared" "$(cat "$LOG")" "its text, or a screenshot if it has too little, goes to your AI provider"
contains "screen: full answer saved" "$(cat "$state/last-answer.md")" $'The error says E0308.\nFix the type.'
# Asking about the screen in words looks at it; a how-to about the screen does not.
: >"$LOG"; bash "$F" ask "what is on my screen right now" >/dev/null 2>&1; sleep 0.4
contains "screen by words: 'what is on my screen' looks at the screen" "$(cat "$LOG")" "context screen --yes --ask what is on my screen right now"
: >"$LOG"; bash "$F" ask "how do I change my screen brightness" >/dev/null 2>&1; sleep 0.4
if grep -q "context screen" "$LOG"; then fail "a how-to about the screen captured it"; else pass "a how-to about the screen does not capture it"; fi
: >"$LOG"; rm -f "$W/again_done"
ACTION_AGAIN=1 PICK="screen follow-up" bash "$F" ask --screen; sleep 0.5
contains "Ask again: screen preserves --screen kind" "$(cat "$LOG")" "friday-ctl-context context screen --yes --ask screen follow-up"
echo "watch an area"
printf '#!/bin/sh\nexit 0\n' >"$S/grim"; chmod +x "$S/grim"
printf '#!/bin/sh\n[ -n "$SLURP_CANCEL" ] && exit 1\necho "10,20 300x200"\n' >"$S/slurp"; chmod +x "$S/slurp"
: >"$LOG"; bash "$F" watch; sleep 0.5
contains "friday watch: the dragged area is watched" "$(cat "$LOG")" "friday-ctl-watch watch region 10,20 300x200"
contains "and it says how to stop" "$(cat "$LOG")" "friday watch stop"
: >"$LOG"; SLURP_CANCEL=1 bash "$F" watch region; sleep 0.3
if grep -q "friday-ctl-watch" "$LOG"; then fail "watched after Esc"; else pass "Esc while dragging watches nothing"; fi
: >"$LOG"; bash "$F" watch stop
contains "friday watch stop stops it" "$(cat "$LOG")" "friday-ctl-watch watch stop"
rm -f "$S/grim"
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
mkdir -p "$HOME/.config/friday"; printf 'engine=ollama\nstate=installing\n' >"$HOME/.config/friday/local-ai.conf"
bash "$F" ask "model refused test"; sleep 0.3
contains "local AI still downloading: says so" "$(cat "$state/last-answer.md")" "Local AI is still downloading"
rm -f "$HOME/.config/friday/local-ai.conf"
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

echo "recall (what was that?)"
: >"$LOG"; out=$(bash "$F" recall on </dev/null)
contains "recall on asks first; no answer changes nothing" "$out" "Nothing changed."
if grep -q "enable --now friday-recall" "$LOG"; then fail "recall turned on without a yes"; else pass "no yes: recall stays off"; fi
contains "and it says where the text goes" "$out" "they go to your cloud provider"
: >"$LOG"; bash "$F" recall on --yes >/dev/null
contains "recall on --yes starts the service" "$(cat "$LOG")" "systemctl --user enable --now friday-recall.service"
rf="$HOME/.local/state/friday/recall.jsonl"; mkdir -p "$(dirname "$rf")"; echo '{"ts":1}' >"$rf"
: >"$LOG"; RECALL_REPLY=$'[kitty — zsh (5 min ago)]\nerror[E0425]: cannot find value cfg' bash "$F" ask "what was that error I saw?"; sleep 0.3
contains "what was that: the kept text is searched" "$(cat "$LOG")" "friday-ctl-recall recall search what was that error I saw?"
contains "and goes with the question (no local model: to the provider)" "$(cat "$LOG")" "text that was on my screen"
: >"$LOG"; RECALL_REPLY="[x]" bash "$F" ask "what is rust"; sleep 0.3
if grep -q "friday-ctl-recall" "$LOG"; then fail "an ordinary question searched recall"; else pass "an ordinary question doesn't search recall"; fi
# With a local model, it answers instead.
envf="$HOME/.config/friday/friday.env"; mkdir -p "$(dirname "$envf")"; cp "$envf" "$W/env.saved2" 2>/dev/null || : >"$W/env.saved2"
printf 'LLAMACPP_ENABLED=1\nLLAMACPP_BASE_URL=http://127.0.0.1:8080/v1\n' >"$envf"; cp "$S/curl" "$W/curl.saved2"
printf '#!/bin/bash\necho "curl $*" >>%q\necho "{\\"choices\\":[{\\"message\\":{\\"content\\":\\"It was E0425 in kitty.\\"}}]}"\n' "$LOG" >"$S/curl"
: >"$LOG"; RECALL_REPLY="[kitty]"$'\n'"error E0425" bash "$F" ask "what was that error?"; sleep 0.3
contains "a local model answers recalled text" "$(cat "$LOG")" "It was E0425 in kitty."
if grep -q "^friday-ctl '" "$LOG"; then fail "recalled text went to the daemon despite a local model"; else pass "recalled text stays local when it can"; fi
cp "$W/env.saved2" "$envf"; mv "$W/curl.saved2" "$S/curl"
contains "recall keep 24h keeps a day" "$(bash "$F" recall keep 24h)" "Recall keeps what it saw for 24 hours."
expect "it says so beside the kept text" test "$(cat "$state/recall-keep")" = 24
contains "7d keeps a week" "$(bash "$F" recall keep 7d)" "for 7 days."
refuse "other keeps are refused" bash "$F" recall keep 1y
bash "$F" recall keep 3h >/dev/null
expect "3h (the default) leaves no file" test ! -e "$state/recall-keep"
bash "$F" recall keep 24h >/dev/null
: >"$LOG"; bash "$F" recall off >/dev/null
contains "recall off stops the service" "$(cat "$LOG")" "systemctl --user disable --now friday-recall.service"
[ -e "$rf" ] && fail "recall off left the kept text" || pass "recall off forgets the kept text"
expect "and the keep goes back to 3 hours" test ! -e "$state/recall-keep"

echo "search a folder you pick"
mkdir -p "$W/notes"
contains "adding a folder asks first" "$(bash "$F" folders add "$W/notes" </dev/null)" "Nothing changed."
: >"$LOG"; bash "$F" folders add "$W/notes" --yes >/dev/null
contains "a folder added is listed" "$(bash "$F" folders list)" "$W/notes"
contains "and indexed" "$(cat "$LOG")" "docs --prepare-folders"
contains "the folder list is owner-only" "$(stat -c %a "$HOME/.config/friday/search-folders")" "600"
bash "$F" folders rm "$W/notes" >/dev/null
contains "rm takes it off" "$(bash "$F" folders list)" "No folders yet"

echo "documentation on this computer"
: >"$LOG"; DOCS_REPLY=$'[Arch wiki: Pacman]\nTo upgrade the system, run pacman -Syu.' bash "$F" ask "how do I upgrade with pacman"; sleep 0.3
contains "a typed question looks in the local docs" "$(cat "$LOG")" "friday-ctl-docs docs -- how do I upgrade with pacman"
contains "what fits goes to the model, named" "$(cat "$LOG")" "Arch wiki: Pacman"
contains "with how to use it" "$(cat "$LOG")" "name the one you use"
: >"$LOG"; bash "$F" ask "write a haiku"; sleep 0.3
if grep -q "These excerpts" "$LOG"; then fail "added docs when none fit"; else pass "nothing fits: the question goes as it is"; fi
: >"$LOG"; DOCS_REPLY="[x]" PRIMARY="some text" PICK="explain" bash "$F" ask --selection; sleep 0.3
if grep -q "friday-ctl-docs" "$LOG"; then fail "looked in the docs for a selection"; else pass "a selection isn't looked up in the docs"; fi
echo "show me"
# A fake FRIDAY Shell: quickshell records the ring it's asked to draw.
mkdir -p "$W/fakeshell"; : >"$W/fakeshell/shell.qml"
printf '#!/bin/bash\necho "quickshell $*" >>"%s"\n' "$LOG" >"$S/quickshell"; chmod +x "$S/quickshell"
printf '#!/bin/bash\n[[ "$*" == "activewindow -j" ]] && printf %%s %q\nexit 0\n' '{"class":"org.gnome.Settings","pid":77,"title":"Settings","at":[0,0]}' >"$S/hyprctl"; chmod +x "$S/hyprctl"
export FRIDAY_SHELL_DIR="$W/fakeshell"
: >"$LOG"; SHOW_REPLY='{"found":true,"name":"Output Devices","x":213,"y":46,"width":131,"height":37}' bash "$F" ask "where's output devices"; sleep 0.3
contains "where's …: FRIDAY looks in the window in front" "$(cat "$LOG")" "friday-ctl-show show where's output devices"
contains "and rings what it found" "$(cat "$LOG")" "ipc call ring point 213 46 131 37 Here: Output Devices"
if grep -q "^friday-ctl '" "$LOG"; then fail "a found control still asked the AI"; else pass "a found control asks no AI"; fi
: >"$LOG"; SHOW_REPLY='{"found":false,"app":"Settings","controls":["Wi-Fi","Appearance","Sound"]}' bash "$F" ask "where is dark mode"; sleep 0.3
contains "no name fits: the model picks from the control names" "$(cat "$LOG")" "friday-ctl-pick"
contains "and its pick is ringed" "$(cat "$LOG")" "ipc call ring point 30 200 180 40 Here: Appearance"
: >"$LOG"; bash "$F" ask "where is the eiffel tower"; sleep 0.3
if grep -q "ring show" "$LOG"; then fail "drew a ring for a question about the world"; else pass "an app FRIDAY can't see: no ring"; fi
contains "and the question is answered in words" "$(cat "$LOG")" "friday-ctl where\\ is\\ the\\ eiffel\\ tower"
printf '#!/bin/bash\n[[ "$*" == "activewindow -j" ]] && printf %%s %q\nexit 0\n' '{"class":"org.keepassxc.KeePassXC","pid":78,"title":"Passwords","at":[0,0]}' >"$S/hyprctl"
: >"$LOG"; SHOW_REPLY='{"found":false,"app":"KeePassXC","controls":["Entry 1"]}' bash "$F" ask "where is my bank entry"; sleep 0.3
if grep -q "friday-ctl-pick" "$LOG"; then fail "sent a private window's controls to the model"; else pass "a private window's controls stay here"; fi
unset FRIDAY_SHELL_DIR; rm -f "$S/hyprctl" "$S/quickshell"

echo "routines"
: >"$LOG"; bash "$F" routine save "morning setup" >/dev/null
contains "friday routine save goes to friday-ctl" "$(cat "$LOG")" "friday-ctl-routine routine save morning setup"
: >"$LOG"; bash "$F" ask "run morning setup"; sleep 0.3
if grep -q "friday-ctl-routine" "$LOG"; then fail "ran a routine that doesn't exist"; else pass "\"run <name>\" with no such routine: an ordinary question"; fi
mkdir -p "$HOME/.local/share/friday/routines"; echo '{}' >"$HOME/.local/share/friday/routines/morning-setup.json"
: >"$LOG"; bash "$F" ask "Run Morning Setup"; sleep 0.3
contains "\"run <name>\" runs the saved routine" "$(cat "$LOG")" "friday-ctl-routine routine run morning setup"
if grep -q "^friday-ctl '" "$LOG"; then fail "running a routine asked the AI"; else pass "running a routine asks no AI"; fi
: >"$LOG"; bash "$F" routine bogus >/dev/null 2>&1 && fail "accepted an unknown routine command" || pass "unknown routine command is refused"
echo '{}' >"$HOME/.local/share/friday/routines/my-setup.json"
: >"$LOG"; bash "$F" ask "run my setup"; sleep 0.3
contains "a routine named \"my setup\" is found with its \"my\"" "$(cat "$LOG")" "friday-ctl-routine routine run my setup"
echo '{}' >"$HOME/.local/share/friday/routines/work.json"
: >"$LOG"; bash "$F" ask "run my work"; sleep 0.3
contains "\"run my work\" means the routine \"work\"" "$(cat "$LOG")" "friday-ctl-routine routine run work"
rm -rf "$HOME/.local/share/friday/routines"

echo "workspaces"
: >"$LOG"; bash "$F" workspace save friday >/dev/null
contains "friday workspace save goes to friday-ctl" "$(cat "$LOG")" "friday-ctl-workspace workspace save friday"
: >"$LOG"; bash "$F" ask "save this as my friday workspace"; sleep 0.3
contains "\"save this as my X workspace\" saves X" "$(cat "$LOG")" "friday-ctl-workspace workspace save friday"
if grep -q "^friday-ctl '" "$LOG"; then fail "saving a workspace asked the AI"; else pass "saving a workspace asks no AI"; fi
: >"$LOG"; bash "$F" ask "restore my friday workspace"; sleep 0.3
if grep -q "workspace restore" "$LOG"; then fail "restored with no workspace saved"; else pass "restore with none saved: an ordinary question"; fi
: >"$LOG"; WS_SAVED=1 bash "$F" ask "restore my friday workspace"; sleep 0.3
contains "\"restore my X workspace\" restores it" "$(cat "$LOG")" "friday-ctl-workspace workspace restore friday"
: >"$LOG"; bash "$F" workspace bogus >/dev/null 2>&1 && fail "accepted an unknown workspace command" || pass "unknown workspace command is refused"
: >"$LOG"; bash "$F" ask "restore my last session"; sleep 0.3
contains "\"restore my last session\" restores it" "$(cat "$LOG")" "friday-ctl-workspace workspace restore last session"
contains "autosave off says how to turn it on" "$(STUB_INACTIVE=1 bash "$F" workspace autosave status)" "friday workspace autosave on"
: >"$LOG"; bash "$F" workspace autosave on >/dev/null
contains "autosave on starts the session timer" "$(cat "$LOG")" "systemctl --user enable --now friday-session-save.timer"
: >"$LOG"; WS_SAVED=1 bash "$F" ask "Friday, let's continue building FRIDAY"; sleep 0.3
contains "\"continue building X\" restores X's workspace" "$(cat "$LOG")" "friday-ctl-workspace workspace restore friday"
contains "and asks with what came back" "$(cat "$LOG")" "opening foot in friday"
: >"$LOG"; bash "$F" ask "continue building the site"; sleep 0.3
contains "with no such workspace it is an ordinary question" "$(cat "$LOG")" "continue\\ building\\ the\\ site"
: >"$LOG"; WS_SAVED=1 bash "$F" ask "continue where we stopped"; sleep 0.3
if grep -q "workspace restore" "$LOG"; then fail "\"continue where we stopped\" restored a workspace called \"where\""; else pass "\"continue where we stopped\" restores nothing by name"; fi

echo "what FRIDAY used"
: >"$LOG"; bash "$F" ask "useditems please"; sleep 0.3
contains "the answer says what the daemon added" "$(cat "$LOG")" "Used: your timeline, a lesson from a failure"
: >"$LOG"; bash "$F" ask "what is the capital of peru"; sleep 0.3
if grep -q "Used:" "$LOG"; then fail "an answer with nothing extra said it used something"; else pass "nothing extra: no used line"; fi

echo "one resume grammar: friday ask and the daemon read the same table"
# timeline.rs checks the daemon against tests/fixtures/resume-phrases.tsv;
# this checks friday_continue_restore against it.
RS="$WORK/resume-stub"; mkdir -p "$RS"
printf '#!/bin/bash\necho "$3"\n' >"$RS/friday-ctl"; chmod +x "$RS/friday-ctl"
resume_bad=""
while IFS=$'\t' read -r ask want; do
  [[ -z "$ask" || "$ask" == \#* ]] && continue
  got=$(PATH="$RS:$PATH" bash -c 'source <(sed -n "/^friday_continue_restore()/,/^}/p" "$1"); friday_continue_restore "$2"' _ "$F" "$ask")
  [[ "$got" == "$want" ]] || resume_bad+=" [$ask: got '$got', want '$want']"
done <"$REPO_ROOT/tests/fixtures/resume-phrases.tsv"
[[ -z "$resume_bad" ]] && pass "every ask in the table restores the project it names (or none)" || fail "resume grammar:$resume_bad"

echo "FRIDAY as an MCP server"
contains "friday mcp says how to connect an app" "$(bash "$F" mcp)" '"command": "friday-ctl", "args": ["mcp"]'
contains "every tool starts not allowed" "$(bash "$F" mcp)" "friday_ask: not allowed"
contains "allow says what it lets apps do" "$(bash "$F" mcp allow friday_ask)" "apps you connect can ask FRIDAY questions"
contains "and it shows as allowed" "$(bash "$F" mcp)" "friday_ask: allowed"
contains "the allow list is owner-only" "$(stat -c %a "$HOME/.config/friday/mcp-allow")" "600"
bash "$F" mcp deny friday_ask >/dev/null
contains "deny takes it back" "$(bash "$F" mcp)" "friday_ask: not allowed"
refuse "an unknown tool can't be allowed" bash "$F" mcp allow rm_rf

echo "that was wrong: a repro without the text"
rm -rf "$state/repros"
: >"$LOG"; bash "$F" ask "what is the capital of peru"; sleep 0.3
expect "each answer leaves its shape in last-ask.json" test -s "$state/last-ask.json"
if grep -q "peru\|Line one" "$state/last-ask.json"; then fail "last-ask.json holds the question or answer"; else pass "last-ask.json holds no question or answer text"; fi
contains "it says how long the question was" "$(cat "$state/last-ask.json")" '"question_chars": 27'
out=$(bash "$F" wrong "Made something up")
contains "friday wrong saves a repro" "$out" "Saved a repro of that answer"
R=$(ls "$state/repros"/*.json | head -n1)
contains "with the reason" "$(cat "$R")" '"reason": "Made something up"'
if grep -q "peru" "$R"; then fail "the repro holds the question"; else pass "the repro holds no question text"; fi
contains "repros are owner-only" "$(stat -c %a "$state/repros")" "700"
contains "--list shows them" "$(bash "$F" wrong --list)" "Made something up · plain"
contains "the report includes them" "$(bash "$F" report "$W/report.txt" >/dev/null 2>&1; cat "$W/report.txt")" '"reason": "Made something up"'
: >"$LOG"; bash "$F" ask "this will fail"; sleep 0.3
contains "a failed answer keeps only the error's kind" "$(cat "$state/last-ask.json")" '"error": "unreachable"'
if grep -q "network unreachable" "$state/last-ask.json"; then fail "last-ask.json holds the error text"; else pass "no error text in last-ask.json"; fi
rm -f "$state/last-ask.json"
contains "nothing to report before an answer" "$(bash "$F" wrong x 2>&1 || true)" "No answer to report yet"

echo "the browser extension"
# friday-ctl browser tab answers as the extension would; a browser is in front.
mkdir -p "$W/bt-bin"
cat >"$W/bt-bin/friday-ctl" <<STUB
#!/bin/bash
if [ "\$1 \$2" = "browser tab" ]; then printf '%s\n' '{"title":"Rust book","url":"https://example.com/book","selection":"","text":"Ownership rules: each value has one owner."}'; exit 0; fi
exec "$S/friday-ctl" "\$@"
STUB
chmod +x "$W/bt-bin/friday-ctl"
printf '#!/bin/bash\n[[ "$*" == "activewindow -j" ]] && printf %%s %q\nexit 0\n' '{"class":"firefox","title":"Rust book — Mozilla Firefox"}' >"$S/hyprctl"; chmod +x "$S/hyprctl"
: >"$LOG"; PATH="$W/bt-bin:$PATH" bash "$F" ask --screen "summarize this page"; sleep 0.3
if grep -q "context screen" "$LOG"; then fail "took a screenshot though the extension had the page"; else pass "a browser with the extension sends the page's text, no screenshot"; fi
contains "with its address" "$(cat "$LOG")" "Address: https://example.com/book"
contains "and its text" "$(cat "$LOG")" "Ownership rules"
contains "the page is marked as quoted material (it can't pick the tool)" "$(cat "$LOG")" "[page]"
: >"$LOG"; bash "$F" ask --screen "summarize this page"; sleep 0.3
contains "without the extension it's a screenshot as before" "$(cat "$LOG")" "context screen"
# A private window the extension can't see: its tab answer is another window's.
printf '#!/bin/bash\n[[ "$*" == "activewindow -j" ]] && printf %%s %q\nexit 0\n' '{"class":"firefox","title":"Mozilla Firefox Private Browsing"}' >"$S/hyprctl"
: >"$LOG"; PATH="$W/bt-bin:$PATH" bash "$F" ask --screen "summarize this page"; sleep 0.3
if grep -q "Ownership rules" "$LOG"; then fail "sent another window's tab"; else pass "a tab that isn't the one in front isn't sent"; fi
rm -f "$S/hyprctl"
H2="$W/bhome"; mkdir -p "$H2"
HOME="$H2" XDG_CONFIG_HOME="$H2/.config" bash "$F" browser setup >/dev/null
contains "setup registers the host with Chromium for FRIDAY's extension only" "$(cat "$H2/.config/chromium/NativeMessagingHosts/org.friday.browser.json")" "chrome-extension://eiblopljacafgajanhkngbhejpjjmfff/"
contains "and with Firefox" "$(cat "$H2/.mozilla/native-messaging-hosts/org.friday.browser.json")" "browser@friday-os"
echo "encrypted sync between your machines"
# A stand-in for age: "encrypts" by naming the recipients; only a key among
# them opens it (the flow is tested here, age itself does the cryptography).
cat >"$S/age-keygen" <<'STUB'
#!/bin/bash
if [ "$1" = -o ]; then echo "SECRET-$RANDOM$RANDOM" >"$2"; exit 0; fi
[ "$1" = -y ] && printf 'age1%s\n' "$(sha256sum "$2" | cut -c1-58)"
STUB
cat >"$S/age" <<'STUB'
#!/bin/bash
if [ "$1" = -d ]; then
  me=$(age-keygen -y "$3"); head -n1 "$4" | grep -qw "$me" || { echo "no identity matched" >&2; exit 1; }
  tail -n +2 "$4"; exit 0
fi
rec=""; out=""
while [ $# -gt 0 ]; do case "$1" in -r) rec="$rec $2"; shift 2 ;; -o) out="$2"; shift 2 ;; *) shift ;; esac; done
{ echo "AGE:$rec"; cat; } >"$out"
STUB
chmod +x "$S/age" "$S/age-keygen"
SYNC="$W/syncdir"; mkdir -p "$SYNC"
mA() { XDG_CONFIG_HOME="$W/A/cfg" XDG_STATE_HOME="$W/A/state" XDG_DATA_HOME="$W/A/data" bash "$F" sync "$@"; }
mB() { XDG_CONFIG_HOME="$W/B/cfg" XDG_STATE_HOME="$W/B/state" XDG_DATA_HOME="$W/B/data" bash "$F" sync "$@"; }
mC() { XDG_CONFIG_HOME="$W/C/cfg" XDG_STATE_HOME="$W/C/state" XDG_DATA_HOME="$W/C/data" bash "$F" sync "$@"; }
if need ssh-keygen; then
contains "sync is off until set up" "$(mA)" "not set up"
LA=$(mA init | sed -n 's/^  friday sync trust //p'); LB=$(mB init | sed -n 's/^  friday sync trust //p'); LC=$(mC init | sed -n 's/^  friday sync trust //p')
contains "the key file is owner-only" "$(stat -c %a "$W/A/cfg/friday/sync/key.txt")" "600"
mA folder "$SYNC" >/dev/null; mB folder "$SYNC" >/dev/null; mC folder "$SYNC" >/dev/null
mA project site on >/dev/null; mB project site on >/dev/null; mC project site on >/dev/null
contains "no bundle until another machine is trusted" "$(mA now 2>&1 || true)" "No other machine is trusted yet"
# shellcheck disable=SC2086
mA trust $LB >/dev/null; mB trust $LA >/dev/null
mkdir -p "$W/A/state/friday" "$W/A/data/friday/workspaces"
printf '%s\n' '{"ts":1,"project":"/home/me/code/site","kind":"decision","text":"move images to a CDN"}' \
  '{"ts":2,"project":"/home/me/code/diary/","kind":"idea","text":"private diary idea"}' >"$W/A/state/friday/thoughts.jsonl"
echo '{"windows":[]}' >"$W/A/data/friday/workspaces/site.json"
echo '{"windows":[]}' >"$W/A/data/friday/workspaces/zzz-other.json"
mA now >/dev/null
if grep -q "diary" "$SYNC"/*.age; then fail "a project that isn't synced went into the bundle"; else pass "only synced projects go in the bundle"; fi
expect "the bundle is signed" test -s "$SYNC/$(cut -d' ' -f1 <<<"$LA").age.sig"
out=$(mB now)
contains "the other machine takes the thoughts" "$out" "took 1 thought(s) and 1 workspace(s)"
contains "and keeps them" "$(cat "$W/B/state/friday/thoughts.jsonl")" "move images to a CDN"
contains "an imported workspace is owner-only" "$(stat -c %a "$W/B/data/friday/workspaces/site.json")" "600"
contains "a second sync adds nothing twice" "$(mB now)" "took 0 thought(s) and 0 workspace(s)"
# Machine C (B doesn't trust it) puts its bundle in A's name: B must not take it.
# shellcheck disable=SC2086
mC trust $LB >/dev/null
mkdir -p "$W/C/state/friday"; echo '{"ts":3,"project":"/x/site","kind":"idea","text":"planted"}' >"$W/C/state/friday/thoughts.jsonl"
mC now >/dev/null
cp "$SYNC/$(cut -d' ' -f1 <<<"$LC").age" "$SYNC/$(cut -d' ' -f1 <<<"$LA").age"
out=$(mB now 2>&1)
contains "a bundle in a trusted machine's name but not signed by it is skipped" "$out" "isn't signed by it; skipped"
if grep -q planted "$W/B/state/friday/thoughts.jsonl"; then fail "took a planted thought"; else pass "nothing planted is taken in"; fi
refuse "a line that isn't a trust line is refused" mA trust not-a-key
else skip "sync (needs ssh-keygen)"; fi
rm -f "$S/age" "$S/age-keygen"
echo "teach a routine by showing it"
RUNTIME_SAVE="${XDG_RUNTIME_DIR:-}"; export XDG_RUNTIME_DIR="$W/rt"; mkdir -p "$XDG_RUNTIME_DIR"
# The recorder (friday-ctl routine record) writes its own pid and listens.
RC="$W/rec-bin"; mkdir -p "$RC"
cat >"$RC/friday-ctl" <<STUB
#!/bin/bash
if [ "\$1 \$2" = "routine record" ]; then echo \$\$ >"\$XDG_RUNTIME_DIR/friday-recording.pid"; exec sleep 30; fi
exec "$S/friday-ctl" "\$@"
STUB
chmod +x "$RC/friday-ctl"; PATH_SAVE="$PATH"; export PATH="$RC:$PATH"
: >"$LOG"; out=$(bash "$F" routine record)
contains "recording starts and says how to stop" "$out" "friday routine stop <name>"
contains "a notification shows it records the whole time" "$(cat "$LOG")" "FRIDAY is recording a routine"
expect "it keeps the recorder's pid" test -s "$XDG_RUNTIME_DIR/friday-recording.pid"
REC_PID=$(cat "$XDG_RUNTIME_DIR/friday-recording.pid")
: >"$LOG"; bash "$F" routine stop morning setup >/dev/null 2>&1 || true
contains "stop turns the recording into a routine" "$(cat "$LOG")" "routine finish morning setup"
# Gone, or a zombie nobody reaps (a container without init): either way stopped.
rec_alive() { local st; st=$(ps -o stat= -p "$REC_PID" 2>/dev/null | cut -c1); [ -n "$st" ] && [ "$st" != Z ]; }
for _ in 1 2 3 4 5 6 7 8 9 10; do rec_alive || break; sleep 0.2; done
if rec_alive; then fail "the recorder kept running"; kill "$REC_PID"; else pass "stop ends the recorder itself"; fi
expect "and forgets the recorder" test ! -e "$XDG_RUNTIME_DIR/friday-recording.pid"
contains "stop when not recording says so" "$(bash "$F" routine stop x 2>&1 || true)" "Not recording"
mkdir -p "$HOME/.local/share/friday/routines"; echo '{}' >"$HOME/.local/share/friday/routines/morning-setup.json"
contains "edit opens the routine's file" "$(EDITOR=echo bash "$F" routine edit Morning Setup)" "friday/routines/morning-setup.json"
refuse "edit refuses a path" bash "$F" routine edit ../../etc/x
rm -rf "$HOME/.local/share/friday/routines"; export XDG_RUNTIME_DIR="$RUNTIME_SAVE" PATH="$PATH_SAVE"

echo "answers know their project"
: >"$LOG"; WS_PROJECT=/home/me/code/project-friday bash "$F" ask "why does the build fail"; sleep 0.3
contains "the saved answer names the project in front" "$(cat "$HOME/.local/state/friday/last-answer.md")" "_project: /home/me/code/project-friday_"
: >"$LOG"; bash "$F" ask "what is the capital of peru"; sleep 0.3
if grep -q "_project:" "$HOME/.local/state/friday/last-answer.md"; then fail "an answer with no project in front named one"; else pass "no project in front: none named"; fi
: >"$LOG"; bash "$F" ask "this will fail"; sleep 0.3
contains "a failed answer is marked for the timeline" "$(cat "$HOME/.local/state/friday/last-answer.md")" "_failed_"
if grep -q "_failed_" <(bash "$F" ask "what is the capital of peru" >/dev/null 2>&1; sleep 0.3; cat "$HOME/.local/state/friday/last-answer.md"); then fail "an answer that worked was marked failed"; else pass "an answer that worked isn't marked failed"; fi

echo "thoughts"
: >"$LOG"; bash "$F" ask "Keep this idea: Memory as an INDEX, not a store."; sleep 0.3
contains "\"keep this idea\" keeps it, as typed" "$(cat "$LOG")" "friday-ctl-thought thought add idea Memory as an INDEX, not a store"
mkdir -p "$HOME/.local/share/friday/captions"
printf '10:02:15 lets ship v0.8 on friday\n10:04:01 sam can you write the release notes\n' >"$HOME/.local/share/friday/captions/2026-11-21-1000.txt"
: >"$LOG"; out=$(bash "$F" meeting --yes)
contains "a meeting becomes decisions" "$(cat "$LOG")" "friday-ctl-thought thought add decision (meeting 2026-11-21-1000, 10:02:15) Ship v0.8 on Friday"
contains "actions" "$(cat "$LOG")" "thought add action (meeting 2026-11-21-1000, 10:04:01) Sam writes the release notes"
contains "and open questions" "$(cat "$LOG")" "thought add question (meeting 2026-11-21-1000, 10:05:30) Do we need a beta?"
if grep -q "chatter" "$LOG"; then fail "kept a line that wasn't a decision, action or question"; else pass "anything else in the reply is dropped"; fi
contains "it says what it kept" "$out" "Kept 3 under the project in front"
: >"$LOG"; out=$(bash "$F" meeting </dev/null)
contains "without --yes and no local model it asks first" "$out" "Nothing sent."
if grep -q "meeting transcript" "$LOG"; then fail "sent a transcript without a yes"; else pass "no transcript sent without a yes"; fi
rm -rf "$HOME/.local/share/friday/captions"
if grep -q "^friday-ctl '" "$LOG"; then fail "keeping an idea asked the AI"; else pass "keeping an idea asks no AI"; fi
: >"$LOG"; bash "$F" ask "we decided to compute the index from the logs"; sleep 0.3
contains "\"we decided …\" keeps a decision" "$(cat "$LOG")" "friday-ctl-thought thought add decision compute the index from the logs"
: >"$LOG"; bash "$F" ask "open question: should threads sync between computers?"; sleep 0.3
contains "\"open question: …\" keeps a question" "$(cat "$LOG")" "friday-ctl-thought thought add question should threads sync between computers"

echo "local commands (no AI)"
printf '#!/bin/sh\necho "wpctl $*" >>"%s"\n[ "$1" = get-volume ] && echo "Volume: 0.55${MUTED:+ [MUTED]}"\nexit 0\n' "$LOG" >"$S/wpctl"; chmod +x "$S/wpctl"
rm -f "$state/undo.jsonl"
: >"$LOG"; bash "$F" ask "Volume 40"; sleep 0.3
contains "volume: set on this computer" "$(cat "$LOG")" "wpctl set-volume @DEFAULT_AUDIO_SINK@ 40%"
contains "volume: says what it did" "$(cat "$state/last-answer.md")" "Volume set to 40%."
contains "local command says nothing was sent" "$(cat "$LOG")" "nothing was sent"
if grep -q "friday-ctl" "$LOG"; then fail "a local command went to the daemon"; else pass "a local command never reaches the daemon"; fi
: >"$LOG"; bash "$F" ask "timer 1 min"; sleep 0.3
contains "timer: set" "$(cat "$state/last-answer.md")" "Timer set for 1 minute."
# Undo puts back what the volume was before (0.55 from get-volume).
: >"$LOG"; out=$(bash "$F" undo)
contains "undo: the volume goes back" "$(cat "$LOG")" "wpctl set-volume @DEFAULT_AUDIO_SINK@ 0.55"
contains "undo: and its mute" "$(cat "$LOG")" "wpctl set-mute @DEFAULT_AUDIO_SINK@ 0"
contains "undo: says what it undid" "$out" "Undid the volume change: volume back to 55%."
contains "undo: then there's nothing left" "$(bash "$F" undo)" "Nothing to undo."
# A window the daemon moved (desktop_tool writes this entry): undo moves it back.
cat >"$S/hyprctl" <<STUB
#!/bin/bash
echo "hyprctl \$*" >>"$LOG"
c=\${CLIENTS:-}; [ -n "\$c" ] || c='[{"address":"0xabc1"}]'
[[ "\$*" == "clients -j" ]] && printf %s "\$c"
[[ -n "\${OLD_HYPR:-}" && "\$*" == "dispatch hl.dsp"* ]] && echo "Invalid dispatcher"
exit 0
STUB
chmod +x "$S/hyprctl"
printf '{"ts":"t","said":"moving kitty to workspace 2","kind":"window","args":["0xabc1","1"]}\n' >"$state/undo.jsonl"
: >"$LOG"; out=$(bash "$F" undo)
contains "undo: a moved window goes back" "$(cat "$LOG")" 'hl.dsp.window.move({ workspace = "1", follow = false, window = "address:0xabc1" })'
contains "undo: and says where" "$out" "Undid moving kitty to workspace 2: it's back on workspace 1."
printf '{"ts":"t","said":"moving kitty to workspace 2","kind":"window","args":["0xabc1","1"]}\n' >"$state/undo.jsonl"
: >"$LOG"; out=$(OLD_HYPR=1 bash "$F" undo)
contains "undo: an older Hyprland gets the classic dispatch" "$(cat "$LOG")" "hyprctl dispatch movetoworkspacesilent 1,address:0xabc1"
printf '{"ts":"t","said":"moving kitty to workspace 2","kind":"window","args":["0xabc1","1"]}\n' >"$state/undo.jsonl"
: >"$LOG"; out=$(CLIENTS='[]' bash "$F" undo || true)
contains "undo: a closed window is said so" "$out" "That window is closed"
printf '{"ts":"t","said":"x","kind":"window","args":["0xabc1","1\\" })"]}\n' >"$state/undo.jsonl"
: >"$LOG"; out=$(bash "$F" undo || true)
if grep -q "dispatch" "$LOG"; then fail "undo dispatched an odd workspace"; else pass "undo: an odd entry is refused"; fi
rm -f "$S/hyprctl" "$state/undo.jsonl"
: >"$LOG"; MUTED=1 bash "$F" ask "unmute"; sleep 0.3; : >"$LOG"; bash "$F" ask "undo"; sleep 0.3
contains "\"undo\" in the ask box undoes too" "$(cat "$LOG")" "wpctl set-mute @DEFAULT_AUDIO_SINK@ 1"
contains "and says so" "$(cat "$state/last-answer.md")" "Undid the unmute: volume back to 55%, muted."
if grep -q "friday-ctl" "$LOG"; then fail "\"undo\" went to the daemon"; else pass "\"undo\" never reaches the daemon"; fi
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
# Live captions and the wake word need that same voice setup.
RUNQ="$W/runq"; mkdir -p "$RUNQ"; chmod 700 "$RUNQ"
contains "captions start off" "$(XDG_RUNTIME_DIR="$RUNQ" bash "$F" captions status)" "Captions: off"
: >"$LOG"; out=$(XDG_RUNTIME_DIR="$RUNQ" bash "$F" captions on); sleep 0.5
contains "captions on starts the listener" "$(cat "$LOG")" "friday-ctl-listen listen captions"
contains "and says what it listens to" "$out" "what you hear"
contains "status shows the notes file" "$(XDG_RUNTIME_DIR="$RUNQ" bash "$F" captions status)" "/notes/2026-11-20-0905.txt"
out=$(XDG_RUNTIME_DIR="$RUNQ" bash "$F" captions off)
contains "captions off says where the notes are" "$out" "Notes: /notes/2026-11-20-0905.txt"
contains "and it is off" "$(XDG_RUNTIME_DIR="$RUNQ" bash "$F" captions status)" "Captions: off"
: >"$LOG"; out=$(XDG_RUNTIME_DIR="$RUNQ" bash "$F" captions toggle --mic); sleep 0.5
contains "toggle turns it on (microphone: --mic goes through)" "$(cat "$LOG")" "listen captions --mic"
contains "and names the microphone" "$out" "the microphone"
XDG_RUNTIME_DIR="$RUNQ" bash "$F" captions toggle >/dev/null
contains "toggle again turns it off" "$(XDG_RUNTIME_DIR="$RUNQ" bash "$F" captions status)" "Captions: off"
: >"$LOG"; out=$(bash "$F" wake on </dev/null)
contains "the wake word asks first and says what it hears" "$out" "listens to your microphone all the time"
contains "and no answer changes nothing" "$out" "Nothing changed."
if grep -q "enable --now friday-wake" "$LOG"; then fail "the wake word turned on without a yes"; else pass "no yes: the wake word stays off"; fi
: >"$LOG"; bash "$F" wake on --yes >/dev/null
contains "wake on --yes starts the service" "$(cat "$LOG")" "systemctl --user enable --now friday-wake.service"
: >"$LOG"; bash "$F" wake off >/dev/null
contains "wake off stops the service" "$(cat "$LOG")" "systemctl --user disable --now friday-wake.service"
rm -f "$HOME/.local/share/friday/whisper/ggml-base.en.bin"
contains "without voice, captions say so" "$(XDG_RUNTIME_DIR="$RUNQ" bash "$F" captions on 2>&1)" "friday voice setup"
contains "without voice, the wake word says so" "$(bash "$F" wake on --yes 2>&1)" "friday voice setup"

echo "face unlock"
contains "face status says it's off and how to turn it on" "$(bash "$F" face status 2>&1)" "friday face setup"
expect "face setup changes nothing when declined" bash -c "printf 'n\\n' | bash '$F' face setup | grep -q 'Nothing changed'"
expect "face rejects an unknown action" bash -c "! bash '$F' face bogus"

echo "screen-text"
contains "screen-text is on by default" "$(bash "$F" screen-text status)" "Screen text: on"
bash "$F" screen-text off >/dev/null
contains "screen-text off is saved" "$(bash "$F" screen-text status)" "Screen text: off"
[[ "$(stat -c %a "$HOME/.config/friday/friday.env")" == 600 ]] && pass "screen-text keeps the env file private" || fail "friday.env is not 0600"
bash "$F" screen-text on >/dev/null
contains "screen-text on is saved" "$(bash "$F" screen-text status)" "Screen text: on"
[[ "$(grep -c '^FRIDAY_SCREEN_TEXT=' "$HOME/.config/friday/friday.env" 2>/dev/null || true)" == 0 ]] && pass "screen-text on (the default) leaves no setting" || fail "FRIDAY_SCREEN_TEXT left in friday.env"

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
grep -v -- 'paste-as' "$REPO_ROOT/iso/friday/airootfs/etc/skel/.config/hypr/bindings.lua" >"$HOME/.config/hypr/bindings.lua"
contains "keys update adds Paste as" "$(bash "$F" keys update)" "Added Paste as"
contains "right after the selection key" "$(grep -A1 'friday ask --selection' "$HOME/.config/hypr/bindings.lua")" '"SUPER + ALT + V", "FRIDAY: Paste as…", "friday paste-as"'
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
# Hermes, with no friday.env yet (no key saved): asked with no tools, then saved.
printf '#!/bin/bash\necho "hermes $*" >>"%s"; echo ready\n' "$LOG" >"$S/hermes"; chmod +x "$S/hermes"
mv "$envf" "$envf.saved" 2>/dev/null || true; : >"$LOG"
expect "connect hermes with no friday.env yet" bash "$F" connect hermes
contains "hermes is asked one-shot with a toolset of no tools" "$(cat "$LOG")" "hermes --ignore-rules -z Reply with the single word: ready -t context_engine"
contains "hermes is saved" "$(cat "$envf" 2>/dev/null)" "FRIDAY_AGENT_CLIS=hermes"
rm -f "$envf"; mv "$envf.saved" "$envf" 2>/dev/null || true
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

# A fake 16 GB machine: the Balanced preset (8 GB) gets gemma3:4b.
hw16="$W/hw16"; mkdir -p "$hw16/proc"
printf 'MemTotal:       16777216 kB\n' >"$hw16/proc/meminfo"
out=$(FRIDAY_HW_ROOT="$hw16" bash "$F" private on 2>&1)
contains "explains answers stay on computer" "$out" "Answers stay on this computer"
contains "explains slower than cloud" "$out" "Slower than cloud models"
contains "explains the model is sized to the computer" "$out" "sized to this computer"
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
contains "no daemon: the real ask is skipped" "$out" "skipped: the daemon isn't listening"
# A listener standing in for the daemon: the real ask goes through friday-ctl.
python3 -c 'import socket,time; s=socket.socket(); s.bind(("127.0.0.1",0)); s.listen(5); print(s.getsockname()[1], flush=True); time.sleep(30)' >"$W/port" &
LISTENER=$!; for _ in 1 2 3 4 5 6 7 8 9 10; do [ -s "$W/port" ] && break; sleep 0.2; done
printf '{"status":"running","token":"t","listen":"127.0.0.1:%s"}' "$(cat "$W/port")" >"$state/state.json"
out=$(bash "$F" doctor 2>&1 || true)
contains "a real ask answers, with who and how fast" "$out" "answered in 0.9 s by kilo/m"
out=$(READY_FAILS=1 bash "$F" doctor 2>&1 || true)
contains "a failed ask says why and where to look" "$out" "no answer: All candidates failed (2 tried). opencode: 429"
contains "and names the next step" "$out" "friday providers"
out=$(READY_TIMES_OUT=1 bash "$F" doctor 2>&1 || true)
contains "a timed-out ask shows friday-ctl's reason" "$out" "no answer: timed out after 60s"
out=$(bash "$F" doctor --quick 2>&1 || true)
if [[ "$out" == *"A real ask"* ]]; then fail "--quick still made a real ask"; else pass "--quick makes no ask"; fi
kill "$LISTENER" 2>/dev/null || true
printf '{"status":"running","token":"t","listen":"127.0.0.1:1"}' >"$state/state.json"
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
