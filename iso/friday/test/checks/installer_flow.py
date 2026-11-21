#!/usr/bin/env python3
"""Drive the live-ISO configurator through a pty with scripted keystrokes.

Covers the paths that once broke: invalid hostname/username and a mismatched
password must re-prompt (not exit), timezone/keymap search must pick real
values, the review screen must allow editing, and the final spec must reach
the installer. Disks, the installer and network are stubbed.

usage: installer_flow.py <configurator> <stub_dir> [multi]

multi: pick Gemini, Groq (no key) and local AI (Ollama, Light) instead of
the default free models.
alongside: the disk has Windows (STUB_WINDOWS=1): install next to it.
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
MULTI = sys.argv[3:] == ["multi"]
ALONGSIDE = sys.argv[3:] == ["alongside"]
UP, DOWN, ENTER, CTRL_U = "\x1b[A", "\x1b[B", "\r", "\x15"

# (seconds to wait for the screen, keys to send)
STEPS = [
    (6, ENTER),  # welcome: Let's begin
    (8, ENTER),  # disk: first (vda)
    *(
        [
            (4, ENTER),  # install alongside Windows (first)
            (3, "y"),  # BitLocker: I have the key
            (4, ENTER),  # size: the suggested one
            (3, ENTER),  # use it
            (3, ENTER),  # Windows starts as now
        ]
        if ALONGSIDE
        else []
    ),
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
    (3, "n"),  # FRIDAY's look? choose my own
    (3, ENTER),  # desktop colours: FRIDAY Shell (default)
    (2, DOWN),
    (1, ENTER),  # window manager: niri
    (2, DOWN),
    (1, ENTER),  # style: Island
    (2, ENTER),  # login theme
    (4, "x"),  # features: desktop control off
    (1, DOWN * 3),
    (1, "x"),  # window text on
    (1, ENTER),
    *(
        [
            (6, DOWN + "x"),  # AI: Gemini
            (1, DOWN + "x"),  # Groq
            (1, DOWN * 6 + "x"),  # Local AI (last)
            (1, ENTER),
            (3, "gem-key-123456" + ENTER),  # Gemini key
            (3, ENTER),  # Groq: no key, skipped
            (5, ENTER),  # engine: Ollama
            (4, ENTER),  # preset: Light (first; no GPU)
        ]
        if MULTI
        else [
            (6, ENTER),  # AI: OpenCode Zen (first, default)
            (3, ENTER),  # no key: free models
        ]
    ),
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
        TERM="xterm-256color",
        FRIDAY_FAST="1",
        FRIDAY_ASSUME_UEFI="1",
        PATH=stub_dir + ":" + os.environ["PATH"],
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
    "review screen shown": (
        "Last check: add FRIDAY next to Windows on /dev/vda"
        if ALONGSIDE
        else "Last check: erase /dev/vda"
    )
    in text,
    "autologin defaults to off": "Password required at login" in text,
    "encryption defaults to on": "Disk will be encrypted (LUKS2)" in text,
    "review shows encryption": "(encrypted)" in text,
    "review lists what FRIDAY may do": "Allowed" in text and "window text" in text,
    "installer received the spec": bool(env_lines),
    "keyboard is chosen before the password": 0
    <= text.find("Keyboard: uk")
    < text.find("Choose a password"),
}
if MULTI:
    checks.update(
        {
            "a provider with no key is skipped, in words": "skipped Groq" in text,
            "the machine is described": "This computer: 16 GB memory" in text,
            "the preset says text only": "qwen3:4b" in text and "text only" in text,
            "review lists every choice": "Gemini (key ••••3456)" in text
            and "Local: qwen3:4b on Ollama (text only, backs up cloud)" in text,
        }
    )
else:
    checks["AI defaults to OpenCode Zen free models"] = "OpenCode Zen free models (no key)" in text
if ALONGSIDE:
    checks.update(
        {
            "the system on the disk is named": "This disk already has Windows on it." in text,
            "BitLocker is warned about": "encrypted with BitLocker" in text,
            "a size is suggested": "I'd use 80 GB" in text,
            "the size is judged": "Good: room for FRIDAY" in text,
            "the review says nothing is erased": "Windows and its files stay as they are" in text,
        }
    )
env = env_lines[-1] if env_lines else ""
ai_spec = (
    [("ai", "[gemini]"), ("keys", "[gemini]"), ("local", "[ollama:qwen3:4b:4096]")]
    if MULTI
    else [("ai", "[]"), ("keys", "[]"), ("local", "[::]")]
)
for key, want in ai_spec + [
    ("disk", "/dev/vda"),
    *(
        [
            ("erase", ""),
            ("mode", "alongside"),
            ("alongside", "/dev/vda"),
            ("size", "81920"),
            ("bootdefault", "other"),
            ("bitlocker_ack", "1"),
        ]
        if ALONGSIDE
        else [("erase", "/dev/vda"), ("mode", "erase")]
    ),
    ("host", "mybox"),
    ("user", "alice"),
    ("pw", "secret1"),
    ("tz", "Asia/Kolkata"),
    ("kb", "uk"),
    ("autologin", "false"),
    ("encrypt", "true"),
    ("interactive", "1"),
    ("session", "niri"),
    ("style", "Island"),
    ("desktop", "false"),
    ("tasks", "true"),
    ("appname", "true"),
    ("screentext", "true"),
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
