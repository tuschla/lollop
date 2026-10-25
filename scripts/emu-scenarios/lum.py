"""Mean luma (0-255) of a raw `screencap` (no -p) read on stdin.

The API 29+ raw format is a 16-byte header (width, height, pixel format,
colour space as little-endian u32) followed by RGBA_8888 rows. Samples every
8th pixel of the band between 15 % and 85 % of the height (skipping status and
navigation bars). Prints "<mean> <width>x<height>".
"""
import struct
import sys

raw = sys.stdin.buffer.read()
w, h, fmt = struct.unpack_from("<III", raw, 0)
hdr = 16 if len(raw) >= 16 + w * h * 4 else 12
tot = n = 0
for y in range(int(h * 0.15), int(h * 0.85), 8):
    row = hdr + y * w * 4
    for x in range(0, w, 8):
        r, g, b = raw[row + x * 4], raw[row + x * 4 + 1], raw[row + x * 4 + 2]
        tot += 0.299 * r + 0.587 * g + 0.114 * b
        n += 1
print(f"{tot / max(n, 1):.0f} {w}x{h}")
