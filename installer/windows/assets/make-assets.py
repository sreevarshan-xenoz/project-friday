#!/usr/bin/env python3
"""Draws the installer's two dialog images (FRIDAY's palette) as 24-bit BMPs.

The committed BMPs are the output; run this to change them:
  python3 installer/windows/assets/make-assets.py
WiX uses them as WixUIBannerBmp (493x58, the strip above the inner pages) and
WixUIDialogBmp (493x312, the welcome and finish pages; the left 164 px show).
"""

import os

from PIL import Image, ImageDraw, ImageFont

NIGHT, ACCENT, SECONDARY, TEXT = (26, 27, 38), (122, 162, 247), (187, 154, 247), (192, 202, 245)
HERE = os.path.dirname(os.path.abspath(__file__))


def font(size: int) -> ImageFont.FreeTypeFont | ImageFont.ImageFont:
    for name in ("DejaVuSans-Bold.ttf", "arialbd.ttf", "LiberationSans-Bold.ttf"):
        try:
            return ImageFont.truetype(name, size)
        except OSError:
            continue
    return ImageFont.load_default()


def lerp(a: tuple[int, int, int], b: tuple[int, int, int], t: float) -> tuple[int, int, int]:
    return (
        int(a[0] + (b[0] - a[0]) * t),
        int(a[1] + (b[1] - a[1]) * t),
        int(a[2] + (b[2] - a[2]) * t),
    )


def gradient(
    w: int, h: int, top: tuple[int, int, int], bottom: tuple[int, int, int]
) -> Image.Image:
    img = Image.new("RGB", (w, h))
    d = ImageDraw.Draw(img)
    for y in range(h):
        d.line([(0, y), (w, y)], fill=lerp(top, bottom, y / max(1, h - 1)))
    return img


def mark(d: ImageDraw.ImageDraw, x: int, y: int, size: int) -> None:
    """FRIDAY's mark: a ring with a bright dot."""
    d.ellipse([x, y, x + size, y + size], outline=ACCENT, width=max(2, size // 9))
    r = size // 5
    c = (x + size * 0.68, y + size * 0.32)
    d.ellipse([c[0] - r, c[1] - r, c[0] + r, c[1] + r], fill=SECONDARY)


def dialog() -> Image.Image:
    img = Image.new("RGB", (493, 312), (255, 255, 255))
    img.paste(gradient(164, 312, NIGHT, (44, 36, 78)), (0, 0))
    d = ImageDraw.Draw(img)
    mark(d, 42, 56, 80)
    f = font(22)
    w = d.textlength("FRIDAY", font=f)
    d.text(((164 - w) / 2, 150), "FRIDAY", fill=TEXT, font=f)
    small = font(11)
    tag = "Press a key. Ask. Done."
    w = d.textlength(tag, font=small)
    d.text(((164 - w) / 2, 182), tag, fill=ACCENT, font=small)
    d.line([(164, 0), (164, 312)], fill=(59, 66, 97), width=1)
    return img


def banner() -> Image.Image:
    img = Image.new("RGB", (493, 58), (255, 255, 255))
    d = ImageDraw.Draw(img)
    mark(d, 493 - 50, 9, 40)
    d.rectangle([0, 55, 493, 57], fill=ACCENT)
    return img


dialog().save(os.path.join(HERE, "dialog.bmp"))
banner().save(os.path.join(HERE, "banner.bmp"))
print("wrote dialog.bmp and banner.bmp")
