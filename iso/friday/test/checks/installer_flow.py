#!/usr/bin/env python3
"""Drive the live-ISO configurator through a pty with scripted keystrokes.

Covers the paths that once broke: invalid hostname/username and a mismatched
password must re-prompt (not exit), timezone/keymap search must pick real
values, the review screen must allow editing, and the final spec must reach
the installer. Disks, the installer and network are stubbed.

usage: installer_flow.py <configurator> <stub_dir>
"""

import fcntl
import os
import pty
import re
import select
import struct
import sys
import termios
import time

configurator, stub_dir = sys.argv[1], sys.argv[2]
UP, DOWN, ENTER, CTRL_U = "\x1b[A", "\x1b[B", "\r", "\x15"

# (seconds to wait for the screen, keys to send)
STEPS = [
    (6, ENTER),  # welcome: Let's begin
    (8, ENTER),  # disk: first (vda)
    (4, "kolkata"),
    (2, ENTER),  # timezone search
    (4, "uk"),
    (2, ENTER),  # keymap search: before any password, which is typed with it
    (3, CTRL_U + "My Box" + ENTER),  # invalid hostname (space)
    (2, CTRL_U + "mybox" + ENTER),
    (2, CTRL_U + "Bob" + ENTER),  # invalid username (uppercase)
    (2, CTRL_U + "alice" + ENTER),
    (2, "abc" + ENTER),
    (2, "abd" + ENTER),  # mismatch
    (2, "secret1" + ENTER),
    (2, "secret1" + ENTER),
    (3, ENTER),  # autologin? default No
    (3, ENTER),  # encrypt the disk? default Yes
    (3, ENTER),  # desktop colours: FRIDAY Shell (default)
    (2, DOWN),
    (1, ENTER),  # window manager: niri
    (2, DOWN),
    (1, ENTER),  # style: Island
    (2, ENTER),  # login theme
    (6, ENTER),  # AI: OpenCode Zen (first, default)
    (3, ENTER),  # no key: free models
    (4, DOWN * 2),
    (1, ENTER),  # review: Change names
    (3, ENTER),
    (2, ENTER),  # keep names
    (4, ENTER),  # review: Install
    (2, "y"),  # confirm erase
    (6, ""),
]

pid, fd = pty.fork()
if pid == 0:
    os.environ.update(
        TERM="xterm-256color", FRIDAY_FAST="1", PATH=stub_dir + ":" + os.environ["PATH"]
    )
    os.execv(configurator, [configurator])

fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 110, 0, 0))
out = b""


def pump(seconds: float) -> None:
    global out
    end = time.time() + seconds
    while time.time() < end:
        ready, _, _ = select.select([fd], [], [], 0.1)
        if ready:
            try:
                out += os.read(fd, 65536)
            except OSError:
                return


for wait, keys in STEPS:
    pump(wait)
    if keys:
        os.write(fd, keys.encode())
pump(2)
os.kill(pid, 9)

text = re.sub(
    r"\x1b\[[0-9;?<>=]*[A-Za-z]|\x1b\][^\x07]*\x07|\x1b[()][A-Z0-9]",
    "",
    out.decode("utf-8", "replace"),
)
env_lines = [line for line in text.splitlines() if "INSTALLER-ENV" in line]

checks = {
    "FRIDAY greets the user": "Hello. I'm FRIDAY." in text,
    "network check passes": "Online." in text,
    "invalid hostname re-prompts": "Use lowercase letters" in text,
    "invalid username re-prompts": "Start with a lowercase letter" in text,
    "password mismatch re-prompts": "didn't match" in text,
    "review screen shown": "Last check: erase /dev/vda" in text,
    "autologin defaults to off": "Password required at login" in text,
    "encryption defaults to on": "Disk will be encrypted (LUKS2)" in text,
    "review shows encryption": "(encrypted)" in text,
    "AI defaults to OpenCode Zen free models": "OpenCode Zen free models (no key)" in text,
    "installer received the spec": bool(env_lines),
    "keyboard is chosen before the password": 0
    <= text.find("Keyboard: uk")
    < text.find("Choose a password"),
}
env = env_lines[-1] if env_lines else ""
for key, want in [
    ("disk", "/dev/vda"),
    ("host", "mybox"),
    ("user", "alice"),
    ("pw", "secret1"),
    ("tz", "Asia/Kolkata"),
    ("kb", "uk"),
    ("ai", "[]"),
    ("autologin", "false"),
    ("encrypt", "true"),
    ("interactive", "1"),
    ("session", "niri"),
    ("style", "Island"),
]:
    checks[f"spec {key}={want}"] = f"{key}={want}" in env

ok = True
for name, good in checks.items():
    print(("  \x1b[32m✓\x1b[0m " if good else "  \x1b[31m✗\x1b[0m ") + name)
    ok &= good
if not ok:
    print("---- screen tail ----")
    print("\n".join(text.splitlines()[-40:]))
sys.exit(0 if ok else 1)
