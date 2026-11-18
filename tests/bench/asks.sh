#!/usr/bin/env bash
# Realistic asks against a real model: how often does FRIDAY get everyday
# questions right? Starts the real daemon in a throwaway state folder with its
# default providers (the free models unless a key is set in the environment),
# asks each question in asks.tsv the way `friday ask` does, and checks the
# answer against what a correct one must contain. Prints a line per question
# and the totals: completed, correct, median time.
#
# Not a CI check: it needs the network and a live model, and a model's bad day
# shows up here. Run it before a release or after a routing change:
#   cargo build -p friday-mobile --bin friday-linux-daemon --bin friday-ctl
#   bash tests/bench/asks.sh [bin-dir]
set -uo pipefail
cd "$(dirname "$0")/../.."
BIN="${1:-target/debug}"
DAEMON_BIN="$BIN/friday-linux-daemon" CTL_BIN="$BIN/friday-ctl"
[[ -x "$DAEMON_BIN" && -x "$CTL_BIN" ]] || { echo "no daemon/friday-ctl in $BIN" >&2; exit 2; }

W=$(mktemp -d)
DAEMON=""
trap 'kill $DAEMON 2>/dev/null; wait 2>/dev/null; rm -rf "$W"' EXIT
env=(env FRIDAY_STATE_FILE="$W/state.json" FRIDAY_MEMORY_DIR="$W/memory" FRIDAY_LISTEN=127.0.0.1:0)
"${env[@]}" "$DAEMON_BIN" >"$W/daemon.log" 2>&1 &
DAEMON=$!
for _ in $(seq 120); do grep -q '"status": *"running"' "$W/state.json" 2>/dev/null && break; sleep 0.5; done
grep -q '"status": *"running"' "$W/state.json" 2>/dev/null || { echo "the daemon never started"; tail -20 "$W/daemon.log"; exit 1; }

total=0 done_n=0 right=0 times=()
while IFS=$'\t' read -r q want; do
  [[ -z "$q" || "$q" == \#* ]] && continue
  total=$((total + 1))
  start=$(date +%s.%N)
  out=$("${env[@]}" timeout 120 "$CTL_BIN" goal --wait "$q" 2>&1)
  secs=$(awk -v a="$start" -v b="$(date +%s.%N)" 'BEGIN { printf "%.1f", b - a }')
  answer=$(python3 -c 'import json,sys
d=json.JSONDecoder(); s=sys.stdin.read(); i=0; last={}
while (j := s.find("{", i)) >= 0:
    try: o, n = d.raw_decode(s[j:]); last = o if isinstance(o, dict) and "outcome" in o else last; i = j + n
    except ValueError: i = j + 1
print(str(last.get("result") or last.get("response") or "") if last.get("outcome") == "completed" else "")' <<<"$out")
  if [[ -z "$answer" ]]; then
    printf '  FAIL  %5ss  did not finish: %s\n' "$secs" "$q"
    continue
  fi
  done_n=$((done_n + 1)); times+=("$secs")
  if grep -qiE -- "$want" <<<"$answer"; then
    right=$((right + 1)); printf '  ok    %5ss  %s\n' "$secs" "$q"
  else
    printf '  WRONG %5ss  %s\n          answer: %s\n' "$secs" "$q" "$(tr '\n' ' ' <<<"$answer" | cut -c1-160)"
  fi
done <"$(dirname "$0")/asks.tsv"

median=$(printf '%s\n' "${times[@]:-0}" | sort -n | awk '{ a[NR] = $1 } END { print (NR ? a[int((NR + 1) / 2)] : 0) }')
echo
echo "completed $done_n/$total, correct $right/$total, median ${median}s"
[[ $right -eq $total ]]
