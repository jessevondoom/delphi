# Draws the app icon: the six-bar mark (spec 111 §4) on the dark window colour, as src-tauri/icons/icon.png (1024 px).
# No dependencies. Run from app/: `python3 -I dev/make-icon.py`, then `dev/make-icns.sh` for the macOS icon set.
import struct
import zlib

SIZE = 1024
BG = (0x1A, 0x18, 0x16)
# orchestrator, strategist, architect / scout, red-team, builder: the console's own colours for those roles.
BARS = [(0xA8, 0x55, 0xF7), (0x14, 0xB8, 0xA6), (0x38, 0xBD, 0xF8), (0xFF, 0xFF, 0x00), (0xEF, 0x44, 0x44), (0xF5, 0x9E, 0x0B)]


def inside_round_rect(x, y, x0, y0, x1, y1, r):
    if x < x0 or x >= x1 or y < y0 or y >= y1:
        return False
    cx = min(max(x, x0 + r), x1 - r - 1)
    cy = min(max(y, y0 + r), y1 - r - 1)
    return (x - cx) ** 2 + (y - cy) ** 2 <= r * r


def pixel(x, y):
    # A rounded square with a margin, as macOS icons have; transparent outside it.
    if not inside_round_rect(x, y, 100, 100, 924, 924, 184):
        return (0, 0, 0, 0)
    bar_w, bar_h, gap_x, gap_y = 200, 108, 32, 56
    left = (SIZE - (3 * bar_w + 2 * gap_x)) // 2
    top = (SIZE - (2 * bar_h + gap_y)) // 2
    for i, c in enumerate(BARS):
        bx = left + (i % 3) * (bar_w + gap_x)
        by = top + (i // 3) * (bar_h + gap_y)
        if inside_round_rect(x, y, bx, by, bx + bar_w, by + bar_h, 30):
            return (*c, 255)
    return (*BG, 255)


def png(rows):
    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body) & 0xFFFFFFFF)

    raw = b"".join(b"\x00" + bytes(v for px in row for v in px) for row in rows)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", SIZE, SIZE, 8, 6, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b"")


if __name__ == "__main__":
    data = png([[pixel(x, y) for x in range(SIZE)] for y in range(SIZE)])
    with open("src-tauri/icons/icon.png", "wb") as f:
        f.write(data)
    print(f"wrote src-tauri/icons/icon.png ({len(data)} bytes)")
