#!/usr/bin/env bash
# Every package the ISO and the installer ask for must exist in the repos
# (a removed package, broadcom-wl, once broke the build with "target not found").
source "$(dirname "$0")/lib.sh"

if [[ "${SKIP_PACKAGES:-0}" == "1" ]]; then skip "package lookups (SKIP_PACKAGES=1)"; exit 0; fi
if ! need pacman; then skip "package lookups (not on Arch)"; exit 0; fi

check_list() { # check_list <label> <pkg...>
  local label="$1"; shift
  local missing=() p
  # One sync-db query for all names; anything not printed is missing.
  local found
  found=$(pacman -Sp --print-format '%n' "$@" 2>/dev/null | sort -u)
  for p in "$@"; do
    # Package groups/provides resolve to other names; fall back to -Si / -Sg.
    grep -qxF -- "$p" <<<"$found" || pacman -Si -- "$p" >/dev/null 2>&1 \
      || pacman -Sg -- "$p" >/dev/null 2>&1 || missing+=("$p")
  done
  if (( ${#missing[@]} )); then fail "$label: not in repos: ${missing[*]}"
  else pass "$label: all $# packages exist"; fi
}

# friday-git comes from the local [friday] repo build-iso.sh builds.
mapfile -t live < <(grep -vE '^\s*(#|$)' "$PROFILE_DIR/packages.x86_64" | grep -vx 'friday-git')
check_list "live ISO (packages.x86_64)" "${live[@]}"

# The installer's pacstrap list: from the pacstrap call to the first line
# without a trailing backslash.
mapfile -t target < <(awk '/^pacstrap_with_retry \/mnt/{on=1} on{print; if ($0 !~ /\\$/) exit}' \
  "$AIROOTFS/root/installer" | tr -d '\\' | tr ' ' '\n' | grep -vE '^(pacstrap_with_retry|/mnt|)$')
if (( ${#target[@]} < 10 )); then
  fail "could not parse the installer's pacstrap list (${#target[@]} names)"
else
  check_list "installed system (installer pacstrap)" "${target[@]}"
fi

finish
