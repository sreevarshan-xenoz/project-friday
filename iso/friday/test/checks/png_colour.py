#!/usr/bin/env python3
"""Count a colour's pixels in part of a PNG (grim output).

Usage: png_colour.py <file.png> <#rrggbb> <top|bottom|x0,y0,x1,y1>
  prints how many pixels of that colour are in the top or bottom half, or in
  the box from (x0, y0) up to (x1, y1)

Standard library only (the checks run on a bare Arch image): 8-bit RGB or
RGBA, non-interlaced, the PNG filters 0-4. A pixel counts when each channel
is within 3 of the colour.
"""

import struct
import sys
import zlib


def pixels(path: str) -> tuple[int, int, int, bytes]:
    """Width, height, bytes per pixel and the unfiltered pixel rows."""
    with open(path, "rb") as f:
        data = f.read()
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        raise ValueError("not a PNG")
    pos, idat = 8, b""
    width = height = bpp = 0
    while pos < len(data):
        (length,) = struct.unpack(">I", data[pos : pos + 4])
        kind = data[pos + 4 : pos + 8]
        body = data[pos + 8 : pos + 8 + length]
        if kind == b"IHDR":
            width, height, depth, colour, _, _, interlace = struct.unpack(">IIBBBBB", body)
            if depth != 8 or colour not in (2, 6) or interlace:
                raise ValueError("only 8-bit RGB/RGBA, non-interlaced")
            bpp = 3 if colour == 2 else 4
        elif kind == b"IDAT":
            idat += body
        pos += 12 + length
    raw = zlib.decompress(idat)
    stride = width * bpp
    out = bytearray(height * stride)
    prev = bytearray(stride)
    for y in range(height):
        ftype = raw[y * (stride + 1)]
        line = bytearray(raw[y * (stride + 1) + 1 : (y + 1) * (stride + 1)])
        for i in range(stride):
            a = line[i - bpp] if i >= bpp else 0
            b = prev[i]
            c = prev[i - bpp] if i >= bpp else 0
            if ftype == 1:
                line[i] = (line[i] + a) & 0xFF
            elif ftype == 2:
                line[i] = (line[i] + b) & 0xFF
            elif ftype == 3:
                line[i] = (line[i] + (a + b) // 2) & 0xFF
            elif ftype == 4:
                p = a + b - c
                pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                pred = a if pa <= pb and pa <= pc else b if pb <= pc else c
                line[i] = (line[i] + pred) & 0xFF
        out[y * stride : (y + 1) * stride] = line
        prev = line
    return width, height, bpp, bytes(out)


def count(path: str, colour: str, area: str) -> int:
    width, height, bpp, px = pixels(path)
    want = bytes.fromhex(colour.lstrip("#"))
    x0, x1 = 0, width
    if area == "top":
        y0, y1 = 0, height // 2
    elif area == "bottom":
        y0, y1 = height // 2, height
    else:
        x0, y0, x1, y1 = (int(v) for v in area.split(","))
        x1, y1 = min(x1, width), min(y1, height)
    n = 0
    for y in range(y0, y1):
        base = y * width * bpp
        for x in range(x0, x1):
            o = base + x * bpp
            if all(abs(px[o + k] - want[k]) <= 3 for k in range(3)):
                n += 1
    return n


if __name__ == "__main__":
    print(count(sys.argv[1], sys.argv[2], sys.argv[3]))
