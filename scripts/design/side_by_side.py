"""Приёмка задач D (docs/TASKS-DESIGN.md): снимок `urfin --screens` рядом со снимком
дизайна той же сцены — один PNG на сцену, слева Урфин, справа дизайн.

    python scripts/design/side_by_side.py <каталог --screens> <каталог вывода> theme-sepia ...

Снимок Урфина приводится к высоте дизайна (760), палитра PNG сжимается — файлы
лежат в git (docs/screens/d-NN/).
"""
import os
import sys

from PIL import Image, ImageDraw

SHOTS, OUT, *SCENES = sys.argv[1:]
DESIGN = os.path.join(os.path.dirname(__file__), "..", "..", "docs", "design", "screens")
os.makedirs(OUT, exist_ok=True)
for scene in SCENES:
    ours = Image.open(os.path.join(SHOTS, f"{scene}.png")).convert("RGB")
    design = Image.open(os.path.join(DESIGN, f"{scene}.png")).convert("RGB")
    h = design.height
    ours = ours.resize((round(ours.width * h / ours.height), h), Image.LANCZOS)
    gap = 16
    sheet = Image.new("RGB", (ours.width + gap + design.width, h + 24), "white")
    sheet.paste(ours, (0, 24))
    sheet.paste(design, (ours.width + gap, 24))
    d = ImageDraw.Draw(sheet)
    d.text((4, 6), f"urfin --screens: {scene}", fill="black")
    d.text((ours.width + gap + 4, 6), f"docs/design/screens/{scene}.png", fill="black")
    path = os.path.join(OUT, f"{scene}.png")
    sheet.quantize(colors=256, method=Image.Quantize.MEDIANCUT).save(path, optimize=True)
    print(path, os.path.getsize(path))
