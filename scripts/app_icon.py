#!/usr/bin/env python3
"""Draw the iOS app icon: a page of faded lines, the current one in ink, and the
caret after it — the writing room, in which only the sentence at hand is lit.

    python3 scripts/app_icon.py        # needs Pillow (pip install pillow)

Writes the light, dark and tinted 1024px variants into the app's asset catalog.
The drawing is plain geometry, not type, so it renders the same on any machine.
Colours are the web client's tokens (web/src/styles/app.css).
"""
from pathlib import Path

from PIL import Image, ImageDraw

OUT = Path(__file__).resolve().parent.parent / "ios/JustType/Resources/Assets.xcassets/AppIcon.appiconset"
SIZE = 1024
SCALE = 4  # draw large, then downsample, for smooth edges

# name: (background top, background bottom, faded line, current line, caret)
VARIANTS = {
    "AppIcon.png": ("#f8f4ec", "#e9dfcd", "#d6cbb7", "#1c1b19", "#6b4f2a"),
    "AppIcon-Dark.png": ("#1e1c1a", "#121110", "#3a352f", "#ece7dd", "#d2ab74"),
    # The system tints a greyscale image: brightness becomes the tint's strength.
    "AppIcon-Tinted.png": ("#000000", "#000000", "#4a4a4a", "#c8c8c8", "#ffffff"),
}

LINE = 80                     # stroke thickness of a line of text
LEFT = 196                    # left margin of the text block
ROWS = [(307, 828), (477, 768), (647, 520)]  # (centre y, right end)
CARET = (568, 537, 620, 757)  # x0, y0, x1, y1 — centred on the last row


def draw(top, bottom, faded, current, caret):
    s = SIZE * SCALE
    img = Image.new("RGB", (s, s))
    # A vertical wash, light at the top, like paper under a lamp.
    t, b = Image.new("RGB", (1, 1), top).getpixel((0, 0)), Image.new("RGB", (1, 1), bottom).getpixel((0, 0))
    px = ImageDraw.Draw(img)
    for y in range(s):
        f = y / (s - 1)
        px.line([(0, y), (s, y)], fill=tuple(round(t[i] + (b[i] - t[i]) * f) for i in range(3)))

    d = ImageDraw.Draw(img)
    k = SCALE
    for i, (cy, right) in enumerate(ROWS):
        colour = current if i == len(ROWS) - 1 else faded
        d.rounded_rectangle(
            [LEFT * k, (cy - LINE / 2) * k, right * k, (cy + LINE / 2) * k],
            radius=LINE / 2 * k, fill=colour,
        )
    x0, y0, x1, y1 = CARET
    d.rounded_rectangle([x0 * k, y0 * k, x1 * k, y1 * k], radius=(x1 - x0) / 2 * k, fill=caret)
    return img.resize((SIZE, SIZE), Image.LANCZOS)


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    for name, colours in VARIANTS.items():
        draw(*colours).save(OUT / name, optimize=True)
        print(OUT / name)


if __name__ == "__main__":
    main()
