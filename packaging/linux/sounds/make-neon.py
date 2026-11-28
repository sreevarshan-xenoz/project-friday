# Minimal UI sounds for FRIDAY's Neon style: short, soft, synthesised here.
# python3 make-neon.py <dir> writes WAVs; then, for each:
#   ffmpeg -i x.wav -c:a libvorbis -q:a 4 neon/x.oga
import os
import sys
import wave
from typing import Any

import numpy as np

# numpy arrays (numpy has no stubs where mypy runs, so plainly Any).
Arr = Any

SR = 48000
out = sys.argv[1]


def env(n: int, attack: float = 0.004, decay: float = 6.0) -> Arr:
    t = np.arange(n) / SR
    a = np.clip(t / attack, 0, 1)
    return a * np.exp(-decay * t)


def tone(f: float, dur: float, decay: float = 8.0, harm: float = 0.15) -> Arr:
    n = int(SR * dur)
    t = np.arange(n) / SR
    s = (
        np.sin(2 * np.pi * f * t)
        + harm * np.sin(2 * np.pi * 2 * f * t)
        + 0.05 * np.sin(2 * np.pi * 3.01 * f * t)
    )
    return s * env(n, decay=decay)


def sweep(f0: float, f1: float, dur: float, decay: float = 10.0) -> Arr:
    n = int(SR * dur)
    f = np.geomspace(f0, f1, n)
    ph = 2 * np.pi * np.cumsum(f) / SR
    return np.sin(ph) * env(n, decay=decay)


def noise_sweep(dur: float, lo: float = 600, hi: float = 5000, up: bool = True) -> Arr:
    n = int(SR * dur)
    x = np.random.default_rng(7).standard_normal(n)
    # crude band-pass moving through the sweep: one-pole filters per block
    out = np.zeros(n)
    y1 = y2 = 0.0
    for i in range(n):
        p = i / n if up else 1 - i / n
        fc = lo * (hi / lo) ** p
        a = np.exp(-2 * np.pi * fc / SR)
        y1 = (1 - a) * x[i] + a * y1  # low-pass
        y2 = (1 - a) * y1 + a * y2
        out[i] = y1 - y2  # rough band
    t = np.arange(n) / SR
    shape = np.sin(np.pi * t / dur) ** 2
    return out * shape * 4


def place(*parts: tuple[float, Arr]) -> Arr:
    total = max(int(SR * o) + len(p) for o, p in parts)
    s = np.zeros(total)
    for o, p in parts:
        i = int(SR * o)
        s[i : i + len(p)] += p
    return s


def save(name: str, s: Arr, peak_db: float = -16) -> None:
    s = s / (np.max(np.abs(s)) + 1e-9) * 10 ** (peak_db / 20)
    tail = np.zeros(int(SR * 0.02))
    s = np.concatenate([s, tail])
    path = os.path.join(out, name + ".wav")
    with wave.open(path, "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(SR)
        w.writeframes((s * 32767).astype(np.int16).tobytes())


save("notify", place((0, tone(1318.5, 0.16, 18)), (0.07, tone(1975.5, 0.22, 14))))
save("open", sweep(700, 1400, 0.07, 30), -20)
save("close", sweep(1400, 700, 0.07, 30), -20)
save("listen", place((0, tone(880, 0.09, 30)), (0.06, tone(1318.5, 0.14, 20))), -18)
save("done", place((0, tone(1046.5, 0.12, 22)), (0.05, tone(1568, 0.25, 12))), -18)
save("error", place((0, tone(392, 0.14, 18, 0.3)), (0.08, tone(311.1, 0.2, 14, 0.3))), -17)
save("wallpaper", place((0, noise_sweep(0.28)), (0.18, tone(2093, 0.12, 25, 0.05) * 0.3)), -19)
