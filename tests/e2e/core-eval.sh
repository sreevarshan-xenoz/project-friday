#!/usr/bin/env bash
# The core ask, measured: 30 fixed questions (tests/e2e/core-eval.tsv) with a
# selection, a page, the screen or nothing, asked of the real daemon with the
# fake model (fake_model.py) as the only provider. Each case passes when the
# model answered, was sent what it needs (the question, the selected text,
# the picture) and never what it mustn't (a key, anything from a private
# window). It measures FRIDAY's own path from keypress to model and back,
# not a model's quality: the model is a fake that echoes the request.
#
# Prints a line per case and the pass rate, and writes the same as Markdown
# to $GITHUB_STEP_SUMMARY when set (and to $EVAL_REPORT when set). Exits 1
# unless every case passes. Linux only (the screen cases fake grim).
#
# Usage: tests/e2e/core-eval.sh [bin-dir]   (default target/debug)
set -euo pipefail
cd "$(dirname "$0")/../.."
BIN="${1:-target/debug}"
DAEMON_BIN="$BIN/friday-linux-daemon" CTL_BIN="$BIN/friday-ctl"
[[ -x "$DAEMON_BIN" && -x "$CTL_BIN" ]] || {
  echo "no daemon/friday-ctl in $BIN (cargo build -p friday-mobile --bin friday-linux-daemon --bin friday-ctl)" >&2
  exit 2
}
PY=$(command -v python3)

W=$(mktemp -d)
FAKE="" DAEMON=""
cleanup() { kill $DAEMON $FAKE 2>/dev/null || true; wait 2>/dev/null || true; }
trap cleanup EXIT

"$PY" tests/e2e/fake_model.py "$W/port" "$W/model.log" &
FAKE=$!
for _ in $(seq 900); do [[ -s "$W/port" ]] && break; sleep 0.1; done
[[ -s "$W/port" ]] || { echo "the fake model never started" >&2; exit 1; }
PORT=$(cat "$W/port")

# A picture from a fake grim; the window in front from a fake hyprctl (a
# password manager when $W/front.json holds one, else none). No session bus
# and an empty runtime folder, so nothing of this desktop is read.
mkdir -p "$W/bin" "$W/run" "$W/tmp" "$W/xdg" "$W/home"
printf '#!/bin/sh\nprintf "\\211PNG\\r\\n\\032\\nEVAL-SCREEN-PIXELS"\n' >"$W/bin/grim"
printf '#!/bin/sh\n[ -s %q ] && cat %q && exit 0\nexit 1\n' "$W/front.json" "$W/front.json" >"$W/bin/hyprctl"
chmod +x "$W/bin/grim" "$W/bin/hyprctl"
run=(env -u HTTPS_PROXY -u HTTP_PROXY -u https_proxy -u http_proxy -u ALL_PROXY -u all_proxy
  -u DBUS_SESSION_BUS_ADDRESS -u WAYLAND_DISPLAY -u HYPRLAND_INSTANCE_SIGNATURE -u NIRI_SOCKET
  PATH="$W/bin:$PATH" HOME="$W/home" XDG_RUNTIME_DIR="$W/run" TMPDIR="$W/tmp"
  XDG_STATE_HOME="$W/xdg/state" XDG_CACHE_HOME="$W/xdg/cache" XDG_CONFIG_HOME="$W/xdg/config" XDG_DATA_HOME="$W/xdg/data"
  FRIDAY_STATE_FILE="$W/state.json" FRIDAY_MEMORY_DIR="$W/memory" FRIDAY_LISTEN=127.0.0.1:0
  LLAMACPP_ENABLED=1 LLAMACPP_VISION=1 LLAMACPP_BASE_URL="http://127.0.0.1:$PORT/v1")
"${run[@]}" "$DAEMON_BIN" >"$W/daemon.log" 2>&1 &
DAEMON=$!
for _ in $(seq 120); do grep -q '"status": *"running"' "$W/state.json" 2>/dev/null && break; sleep 0.5; done
grep -q '"status": *"running"' "$W/state.json" 2>/dev/null || { echo "the daemon never started" >&2; tail -20 "$W/daemon.log" >&2; exit 1; }

# The last {"outcome": …} object friday-ctl printed, as "outcome<TAB>text".
cat >"$W/outcome.py" <<'PY'
import json, sys
d = json.JSONDecoder(); s = sys.stdin.read(); i = 0; last = {}
while (j := s.find("{", i)) >= 0:
    try:
        o, n = d.raw_decode(s[j:]); i = j + n
    except ValueError:
        i = j + 1; continue
    if isinstance(o, dict) and ("outcome" in o or "error" in o or "response" in o):
        last = o
text = str(last.get("result") or last.get("response") or last.get("error") or "")
kind = last.get("outcome") or ("error" if "error" in last else "answered" if "response" in last else "")
print(f"{kind}\t{text}".replace("\n", " "))
PY
outcome() { "$PY" "$W/outcome.py"; }

total=0 passed=0 rows=()
while IFS=$'\t' read -r kind question context sees never; do
  [[ -z "$kind" || "$kind" == \#* ]] && continue
  total=$((total + 1))
  context="${context//\\n/$'\n'}"
  : >"$W/model.log"
  rm -f "$W/front.json"
  case "$kind" in
    plain) out=$("${run[@]}" timeout 120 "$CTL_BIN" goal --wait --new-conversation "$question" </dev/null 2>&1) || true ;;
    selection)
      goal=$(printf '%s\n\nUse this text as context:\n"""\n%s\n"""' "$question" "$context")
      out=$("${run[@]}" timeout 120 "$CTL_BIN" goal --wait --new-conversation "$goal" </dev/null 2>&1) || true ;;
    page)
      goal=$(printf '%s\n\n[page]\nThe page in front (read from the browser when asked):\nTitle: Example\nAddress: https://example.com/\nText:\n"""\n%s\n"""' "$question" "$context")
      out=$("${run[@]}" timeout 120 "$CTL_BIN" goal --wait --new-conversation "$goal" </dev/null 2>&1) || true ;;
    screen) out=$("${run[@]}" timeout 120 "$CTL_BIN" context screen --yes --ask "$question" </dev/null 2>&1) || true ;;
    private)
      printf '%s' '{"class":"org.keepassxc.KeePassXC","title":"EVAL-PRIVATE.kdbx","at":[0,0],"size":[800,600]}' >"$W/front.json"
      out=$("${run[@]}" timeout 120 "$CTL_BIN" context screen --yes --ask "$question" </dev/null 2>&1) || true ;;
    *) echo "unknown kind: $kind" >&2; exit 2 ;;
  esac
  IFS=$'\t' read -r result text <<<"$(outcome <<<"$out")"
  why=""
  if [[ "$kind" == private ]]; then
    [[ "${text,,}" == *private* ]] || why="not refused as private"
    [[ ! -s "$W/model.log" ]] || why="${why:+$why; }the model was asked"
  else
    [[ "$text" == FAKE-MODEL-ANSWER:* ]] || why="no answer from the model (${result:-no outcome}: ${text:0:80})"
    if [[ "$sees" != - ]] && ! grep -qF -- "$sees" "$W/model.log"; then why="${why:+$why; }the model wasn't sent \"$sees\""; fi
    if [[ "$kind" == screen ]] && ! grep -qF '"images": ["' "$W/model.log"; then why="${why:+$why; }no picture reached the model"; fi
  fi
  if [[ "$never" != - ]] && grep -qF -- "$never" "$W/model.log"; then why="${why:+$why; }the model was sent \"$never\""; fi
  if [[ -z "$why" ]]; then
    passed=$((passed + 1)); printf '  ok    %-9s %s\n' "$kind" "$question"; rows+=("| ✓ | $kind | $question | |")
  else
    printf '  FAIL  %-9s %s: %s\n' "$kind" "$question" "$why"; rows+=("| ✗ | $kind | $question | ${why//|//} |")
  fi
done <tests/e2e/core-eval.tsv

pct=$((passed * 100 / total))
echo
echo "core ask: $passed/$total passed ($pct%), fake model"
report() {
  echo "### Core ask: $passed/$total passed ($pct%)"
  echo
  echo "The real daemon, with a fake model that echoes the request: this measures FRIDAY's path (what reaches the model, what never does), not a model's answers."
  echo
  echo "| | Kind | Question | Why it failed |"
  echo "|---|---|---|---|"
  printf '%s\n' "${rows[@]}"
}
[[ -z "${GITHUB_STEP_SUMMARY:-}" ]] || report >>"$GITHUB_STEP_SUMMARY"
[[ -z "${EVAL_REPORT:-}" ]] || report >"$EVAL_REPORT"
[[ $passed -eq $total ]]
