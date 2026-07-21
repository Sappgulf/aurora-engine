#!/usr/bin/env python3
"""Generate Last Light's compact building-command icon atlas.

The tactical command card deliberately uses a different visual contract from
the large world structure atlas: icons must read at roughly 18--30 world
pixels, survive a dark translucent panel, and retain enough negative space for
bitmap labels.  This local Pillow generator draws six transparent symbols in
a fixed 3x2 grid and also keeps a 4x source sheet for review.  No network or
image-generation service is required, so the output is reproducible in asset
validation and browser builds.

Atlas layout (3 columns x 2 rows, 256 px runtime cells):

    relay pulse | reactor core | fabricator queue
    resource    | field beacon | repair tool
"""

from __future__ import annotations

import math
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter


ROOT = Path(__file__).resolve().parents[3]
OUT = ROOT / "games/last-light/assets/building-command-atlas-v001.png"
SOURCE = ROOT / "tools/asset-sources/last-light/building-command-atlas-v001-source.png"

SCALE = 4
CELL = 256
COLS = 3
ROWS = 2
ATLAS_SIZE = (CELL * COLS, CELL * ROWS)

STEEL_DARK = (13, 23, 34, 255)
STEEL = (40, 64, 80, 255)
STEEL_LIGHT = (99, 134, 147, 255)
CYAN = (63, 235, 226, 245)
CYAN_SOFT = (128, 255, 244, 160)
AMBER = (255, 187, 66, 245)
AMBER_SOFT = (255, 222, 127, 165)
VIOLET = (177, 104, 255, 245)
VIOLET_SOFT = (218, 178, 255, 160)
MAGENTA = (245, 79, 161, 245)
WHITE = (226, 245, 240, 235)


def s(value: float) -> int:
    return round(value * SCALE)


def pt(value: tuple[float, float]) -> tuple[int, int]:
    return s(value[0]), s(value[1])


def tile() -> Image.Image:
    return Image.new("RGBA", (CELL * SCALE, CELL * SCALE), (0, 0, 0, 0))


def line(draw: ImageDraw.ImageDraw, points, fill, width: float = 1.0, joint: str = "curve") -> None:
    draw.line([pt(p) for p in points], fill=fill, width=max(1, s(width)), joint=joint)


def polygon(draw: ImageDraw.ImageDraw, points, fill, outline=None, width: float = 1.0) -> None:
    draw.polygon([pt(p) for p in points], fill=fill)
    if outline is not None:
        line(draw, [*points, points[0]], outline, width)


def rounded(draw: ImageDraw.ImageDraw, box, radius: float, fill, outline=None, width: float = 1.0) -> None:
    draw.rounded_rectangle(tuple(s(v) for v in box), radius=s(radius), fill=fill, outline=outline, width=max(1, s(width)))


def ellipse(draw: ImageDraw.ImageDraw, box, fill, outline=None, width: float = 1.0) -> None:
    draw.ellipse(tuple(s(v) for v in box), fill=fill, outline=outline, width=max(1, s(width)))


def arc(draw: ImageDraw.ImageDraw, box, start: float, end: float, fill, width: float = 1.0) -> None:
    draw.arc(tuple(s(v) for v in box), start, end, fill=fill, width=max(1, s(width)))


def glow_line(base: Image.Image, points, color, width: float) -> None:
    glow = Image.new("RGBA", base.size, (0, 0, 0, 0))
    glow_draw = ImageDraw.Draw(glow)
    line(glow_draw, points, (*color[:3], 100), width * 3.0)
    base.alpha_composite(glow.filter(ImageFilter.GaussianBlur(s(5))))
    line(ImageDraw.Draw(base), points, color, width)


def relay_pulse() -> Image.Image:
    image = tile()
    draw = ImageDraw.Draw(image)
    center = (128, 128)
    for radius, alpha in ((94, 28), (73, 46), (51, 68)):
        arc(draw, (center[0] - radius, center[1] - radius, center[0] + radius, center[1] + radius), 20, 152, (*CYAN[:3], alpha), 4)
        arc(draw, (center[0] - radius, center[1] - radius, center[0] + radius, center[1] + radius), 200, 332, (*AMBER[:3], alpha), 4)
    polygon(draw, [(128, 42), (186, 83), (186, 166), (128, 211), (70, 166), (70, 83)], STEEL_DARK, CYAN, 5)
    polygon(draw, [(128, 65), (161, 91), (161, 157), (128, 183), (95, 157), (95, 91)], STEEL, STEEL_LIGHT, 3)
    ellipse(draw, (112, 112, 144, 144), AMBER, STEEL_DARK, 3)
    ellipse(draw, (121, 121, 135, 135), CYAN_SOFT)
    return image


def reactor_core() -> Image.Image:
    image = tile()
    draw = ImageDraw.Draw(image)
    center = (128, 128)
    for radius, alpha in ((104, 22), (82, 36), (60, 58)):
        arc(draw, (center[0] - radius, center[1] - radius, center[0] + radius, center[1] + radius), 200, 340, (*VIOLET[:3], alpha), 4)
        arc(draw, (center[0] - radius, center[1] - radius, center[0] + radius, center[1] + radius), 20, 160, (*AMBER[:3], alpha), 4)
    polygon(draw, [(128, 36), (187, 75), (205, 145), (161, 207), (95, 207), (51, 145), (69, 75)], STEEL_DARK, VIOLET, 5)
    polygon(draw, [(128, 59), (166, 86), (176, 139), (150, 179), (106, 179), (80, 139), (90, 86)], STEEL, STEEL_LIGHT, 3)
    glow_line(image, [(128, 79), (128, 177)], AMBER, 5)
    glow_line(image, [(92, 128), (164, 128)], CYAN, 4)
    ellipse(draw, (109, 109, 147, 147), AMBER_SOFT, STEEL_DARK, 3)
    ellipse(draw, (119, 119, 137, 137), WHITE)
    return image


def fabricator_queue() -> Image.Image:
    image = tile()
    draw = ImageDraw.Draw(image)
    rounded(draw, (42, 61, 214, 195), 14, STEEL_DARK, CYAN, 5)
    rounded(draw, (59, 79, 197, 178), 8, STEEL, STEEL_LIGHT, 3)
    # Three queue slots plus an assembling unit silhouette.
    for index, color in enumerate((CYAN, AMBER, VIOLET)):
        x = 77 + index * 42
        rounded(draw, (x - 13, 95, x + 13, 121), 5, (8, 22, 31, 255), color, 2)
        ellipse(draw, (x - 5, 103, x + 5, 113), color)
    polygon(draw, [(127, 140), (149, 155), (142, 175), (112, 175), (105, 155)], STEEL_DARK, WHITE, 3)
    line(draw, [(127, 148), (127, 169)], CYAN_SOFT, 3)
    line(draw, [(116, 157), (138, 157)], CYAN_SOFT, 3)
    return image


def resource_icon() -> Image.Image:
    image = tile()
    draw = ImageDraw.Draw(image)
    for radius, alpha in ((100, 22), (78, 38), (57, 62)):
        arc(draw, (128 - radius, 128 - radius, 128 + radius, 128 + radius), 9, 171, (*CYAN[:3], alpha), 4)
        arc(draw, (128 - radius, 128 - radius, 128 + radius, 128 + radius), 189, 351, (*VIOLET[:3], alpha), 4)
    polygon(draw, [(128, 42), (190, 92), (165, 183), (91, 183), (66, 92)], STEEL_DARK, CYAN, 4)
    polygon(draw, [(128, 68), (162, 98), (148, 151), (108, 151), (94, 98)], STEEL, STEEL_LIGHT, 3)
    polygon(draw, [(128, 81), (148, 112), (128, 101), (108, 112)], AMBER, STEEL_DARK, 2)
    ellipse(draw, (116, 116, 140, 140), WHITE)
    return image


def field_beacon() -> Image.Image:
    image = tile()
    draw = ImageDraw.Draw(image)
    for radius, alpha in ((97, 26), (72, 48), (50, 66)):
        arc(draw, (128 - radius, 128 - radius, 128 + radius, 128 + radius), 0, 360, (*AMBER[:3], alpha), 3)
    polygon(draw, [(128, 37), (171, 102), (157, 191), (99, 191), (85, 102)], STEEL_DARK, AMBER, 5)
    line(draw, [(128, 63), (128, 181)], AMBER_SOFT, 4)
    line(draw, [(98, 112), (158, 112)], CYAN_SOFT, 3)
    line(draw, [(102, 151), (154, 151)], CYAN_SOFT, 3)
    ellipse(draw, (114, 99, 142, 127), CYAN, STEEL_DARK, 3)
    polygon(draw, [(128, 106), (136, 119), (128, 115), (120, 119)], WHITE)
    return image


def repair_tool() -> Image.Image:
    image = tile()
    draw = ImageDraw.Draw(image)
    rounded(draw, (47, 92, 178, 164), 10, STEEL_DARK, CYAN, 5)
    rounded(draw, (67, 110, 160, 146), 6, STEEL, STEEL_LIGHT, 3)
    # A compact tool arm and repair beam: this reads as an Engineer command
    # while remaining distinct from the fabricator's square queue silhouette.
    line(draw, [(158, 128), (193, 91), (219, 102)], STEEL_LIGHT, 11)
    glow_line(image, [(193, 91), (219, 102)], CYAN, 4)
    ellipse(draw, (208, 91, 230, 113), AMBER, STEEL_DARK, 3)
    for index, x in enumerate((84, 108, 132)):
        ellipse(draw, (x - 7, 120, x + 7, 134), (CYAN if index % 2 == 0 else AMBER_SOFT))
    line(draw, [(54, 186), (204, 186)], VIOLET_SOFT, 3)
    return image


def main() -> None:
    cells = [relay_pulse(), reactor_core(), fabricator_queue(), resource_icon(), field_beacon(), repair_tool()]
    source = Image.new("RGBA", (CELL * COLS * SCALE, CELL * ROWS * SCALE), (5, 12, 20, 255))
    atlas = Image.new("RGBA", (CELL * COLS * SCALE, CELL * ROWS * SCALE), (0, 0, 0, 0))
    for index, cell in enumerate(cells):
        x = (index % COLS) * CELL * SCALE
        y = (index // COLS) * CELL * SCALE
        atlas.alpha_composite(cell, (x, y))
        # Source sheet gets a neutral review background so all six silhouettes
        # remain visible to an artist; runtime output stays fully transparent.
        source.alpha_composite(cell, (x, y))
    OUT.parent.mkdir(parents=True, exist_ok=True)
    SOURCE.parent.mkdir(parents=True, exist_ok=True)
    atlas = atlas.resize(ATLAS_SIZE, Image.Resampling.LANCZOS)
    source = source.resize((ATLAS_SIZE[0] * SCALE, ATLAS_SIZE[1] * SCALE), Image.Resampling.LANCZOS)
    atlas.save(OUT, format="PNG", optimize=True)
    source.save(SOURCE, format="PNG", optimize=True)
    print(f"wrote {OUT} ({atlas.width}x{atlas.height})")
    print(f"wrote {SOURCE} ({source.width}x{source.height})")


if __name__ == "__main__":
    main()
