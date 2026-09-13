#!/usr/bin/env bash
for cmd in "$@"; do
    [[ -z "$cmd" ]] && continue
    eval "command -v ${cmd%% *}" >/dev/null 2>&1 || continue
    eval "$cmd" &
    exit 0
done

# Graceful terminal fallback if primary GUI app is not yet installed
case "$*" in
    *google-chrome*|*firefox*|*browser*)
        if command -v foot >/dev/null 2>&1; then
            foot -T "FRIDAY Browser Helper" sh -c "echo '● FRIDAY OS: No graphical browser installed yet.'; echo 'To install one: sudo pacman -S firefox (or chromium)'; echo 'Press Enter to open shell...'; read; exec ${SHELL:-/bin/zsh}" &
        fi
        ;;
    *dolphin*|*nautilus*|*thunar*)
        if command -v foot >/dev/null 2>&1; then
            foot -T "FRIDAY File Manager" sh -c "echo '● FRIDAY OS File Browser (cwd: $PWD)'; ls -la; exec ${SHELL:-/bin/zsh}" &
        fi
        ;;
esac
