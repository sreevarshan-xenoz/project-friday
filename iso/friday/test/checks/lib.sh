# shellcheck shell=bash
# Tiny assertion helpers shared by the check groups. Sourced.

FAILS=0
pass() { printf '  \e[32m✓\e[0m %s\n' "$*"; }
fail() { printf '  \e[31m✗\e[0m %s\n' "$*"; FAILS=$((FAILS + 1)); }
skip() { printf '  \e[33m-\e[0m %s (skipped)\n' "$*"; }

# expect <description> <command...>   → pass if the command succeeds
expect() { local d="$1"; shift; if "$@" >/dev/null 2>&1; then pass "$d"; else fail "$d"; fi; }
# refuse <description> <command...>   → pass if the command fails
refuse() { local d="$1"; shift; if "$@" >/dev/null 2>&1; then fail "$d"; else pass "$d"; fi; }
# contains <description> <haystack> <needle>
contains() { if [[ "$2" == *"$3"* ]]; then pass "$1"; else fail "$1 (missing: $3)"; fi; }

need() { local c; for c in "$@"; do command -v "$c" >/dev/null 2>&1 || return 1; done; }

finish() { (( FAILS == 0 )); }
