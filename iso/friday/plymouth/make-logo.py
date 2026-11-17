#!/usr/bin/env python3
"""Draws airootfs/usr/share/friday-installer/plymouth-logo.png, the FRIDAY mark shown by the boot splash (Omarchy's Plymouth
theme with its logo swapped). Run from anywhere: python3 make-logo.py"""
from pathlib import Path
from PIL import Image, ImageDraw, ImageFont

W, H = 800, 188
S = 4  # draw big, then shrink for smooth edges
ACCENT, SIGNAL, TEXT = (255, 143, 76), (95, 211, 188), (236, 232, 226)
font = ImageFont.truetype(str(Path(__file__).parents[1] / "airootfs/usr/share/fonts/redhat/RedHatDisplay-Bold.otf"), 96 * S)

img = Image.new("RGBA", (W * S, H * S), (0, 0, 0, 0))
d = ImageDraw.Draw(img)
word = "F R I D A Y"
tw = d.textlength(word, font=font)
orb = 120 * S
gap = 44 * S
x0 = (W * S - (orb + gap + tw)) / 2
cy = H * S / 2
cx = x0 + orb / 2
d.ellipse([cx - orb / 2, cy - orb / 2, cx + orb / 2, cy + orb / 2], outline=(*ACCENT, 110), width=7 * S)
d.arc([cx - orb / 2, cy - orb / 2, cx + orb / 2, cy + orb / 2], -90, 30, fill=SIGNAL, width=8 * S)
r = orb * 0.2
d.ellipse([cx - r, cy - r, cx + r, cy + r], fill=ACCENT)
d.text((x0 + orb + gap, cy), word, font=font, fill=TEXT, anchor="lm")
img.resize((W, H), Image.LANCZOS).save(Path(__file__).parents[1] / "airootfs/usr/share/friday-installer/plymouth-logo.png", optimize=True)
