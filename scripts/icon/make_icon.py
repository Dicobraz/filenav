"""Генератор иконки Урфина (вариант 3d, утверждён 2026-10-20).

Исходник — этот скрипт: геометрия каждого размера подогнана под пиксельную
сетку отдельно (16, 24, 32, 48; 64 — основная, из неё 64+).
Пишет в assets/icon: urfin*.svg, urfin.ico (16/20/24/32/40/48/64/256,
каждый кадр — из своей сетки), urfin-256.png (README).

    pip install cairosvg pillow
    python scripts/icon/make_icon.py
"""

import os
import struct

import cairosvg
from PIL import Image

OUT = os.path.join(os.path.dirname(__file__), "..", "..", "assets", "icon")

# BAR2 = BAR с непрозрачностью 70 % поверх TILE, но сплошным цветом:
# полупрозрачный столбик под непрозрачной дугой давал светлый «шов».
TILE, BAR, BAR2, EDGE = "#1E2A36", "#6CC4F2", "#5596BA", "#33465A"
G = {
    64: dict(trx=14, lx=16.5, rx=37.5, bw=10, by=13, bh=27, brx=5, cx=32, cy=38, r=10.5, sw=5, n=(18.5, 21, 6, 4, 1), tb=1),
    48: dict(trx=10.5, lx=11, rx=29, bw=8, by=9, bh=20, brx=4, cx=24, cy=28, r=9, sw=4, n=(12.5, 15, 5, 3, 1), tb=1),
    32: dict(trx=7, lx=7, rx=20, bw=5, by=6, bh=13, brx=2.5, cx=16, cy=18.5, r=6.5, sw=3, n=(8, 10, 3, 2, 0.5), tb=1),
    24: dict(trx=5, lx=5, rx=15, bw=4, by=4, bh=10, brx=2, cx=12, cy=13.5, r=5, sw=3, n=None, tb=0),
    16: dict(trx=3.5, lx=3, rx=10, bw=3, by=3, bh=7, brx=1.5, cx=8, cy=9.5, r=3.5, sw=2, n=None, tb=0),
}
# Из какой сетки рисуется каждый размер.
SRC = {16: 16, 20: 24, 24: 24, 32: 32, 40: 48, 48: 48, 64: 64, 256: 64}
ICO = [16, 20, 24, 32, 40, 48, 64, 256]


def svg(u: int) -> str:
    g = G[u]
    h = g["tb"] / 2
    r, cx, cy = g["r"], g["cx"], g["cy"]
    s = [
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{u}" height="{u}" viewBox="0 0 {u} {u}">',
        "<title>Urfin</title>",
        f'<rect x="{h}" y="{h}" width="{u - g["tb"]}" height="{u - g["tb"]}" rx="{g["trx"]}" fill="{TILE}"'
        + (f' stroke="{EDGE}" stroke-width="{g["tb"]}"/>' if g["tb"] else "/>"),
        f'<rect x="{g["lx"]}" y="{g["by"]}" width="{g["bw"]}" height="{g["bh"]}" rx="{g["brx"]}" fill="{BAR}"/>',
        f'<rect x="{g["rx"]}" y="{g["by"]}" width="{g["bw"]}" height="{g["bh"]}" rx="{g["brx"]}" fill="{BAR2}"/>',
        f'<path d="M{cx - r} {cy}a{r} {r} 0 0 0 {r} {r}" fill="none" stroke="{BAR}" stroke-width="{g["sw"]}"/>',
        f'<path d="M{cx} {cy + r}a{r} {r} 0 0 0 {r} {-r}" fill="none" stroke="{BAR2}" stroke-width="{g["sw"]}"/>',
    ]
    if g["n"]:
        x, y, w, hh, rr = g["n"]
        s.append(f'<rect x="{x}" y="{y}" width="{w}" height="{hh}" rx="{rr}" fill="{TILE}" fill-opacity="0.55"/>')
    s.append("</svg>")
    return "\n".join(s) + "\n"


def main() -> None:
    os.makedirs(OUT, exist_ok=True)
    svgs = {u: svg(u) for u in G}
    for u, text in svgs.items():
        name = "urfin.svg" if u == 64 else f"urfin-{u}.svg"
        with open(os.path.join(OUT, name), "w", encoding="utf-8", newline="\n") as f:
            f.write(text)

    png = {px: cairosvg.svg2png(bytestring=svgs[u].encode(), output_width=px, output_height=px) for px, u in SRC.items()}

    # ICO с PNG-кадрами (Vista+).
    blobs = [png[s] for s in ICO]
    head = struct.pack("<HHH", 0, 1, len(blobs))
    off = 6 + 16 * len(blobs)
    dirs, data = b"", b""
    for s, b in zip(ICO, blobs):
        dirs += struct.pack("<BBBBHHII", s % 256, s % 256, 0, 0, 1, 32, len(b), off + len(data))
        data += b
    with open(os.path.join(OUT, "urfin.ico"), "wb") as f:
        f.write(head + dirs + data)

    with open(os.path.join(OUT, "urfin-256.png"), "wb") as f:
        f.write(png[256])

    sizes = sorted(Image.open(os.path.join(OUT, "urfin.ico")).info["sizes"])
    assert sizes == [(s, s) for s in ICO], sizes
    print("ok:", sizes)


if __name__ == "__main__":
    main()
