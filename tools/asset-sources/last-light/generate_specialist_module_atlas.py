#!/usr/bin/env python3
"""Generate Last Light's transparent specialist-module icon atlas.

The briefing already exposes Mara, Ivo, Sena, and Olan as distinct campaign
choices. These icons give those choices a readable visual language without
adding a runtime dependency on an image service. Geometry is deterministic
local Pillow work and the high-resolution source is retained for review.

Atlas layout (4 columns x 2 rows, 256 px runtime cells):

    Mara rescue | Ivo rigger | Sena deep-scan | Olan lattice
    Mara rapid  | Ivo smith  | Sena ghost-mark | Olan decoder
"""

from __future__ import annotations

import math
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter


ROOT = Path(__file__).resolve().parents[3]
OUT = ROOT / "games/last-light/assets/specialist-module-atlas-v001.png"
SOURCE = ROOT / "tools/asset-sources/last-light/specialist-module-atlas-v001-source.png"

SCALE = 4
CELL = 256
COLS = 4
ROWS = 2

STEEL_DARK = (10, 24, 38, 238)
STEEL = (35, 63, 83, 250)
CYAN = (74, 236, 231, 235)
CYAN_SOFT = (125, 255, 246, 135)
AMBER = (247, 175, 58, 238)
AMBER_SOFT = (255, 207, 109, 150)
VIOLET = (180, 125, 255, 235)
GREEN = (108, 244, 133, 235)
MAGENTA = (244, 72, 168, 235)
IVORY = (222, 233, 226, 230)


def s(value: float) -> int:
    return round(value * SCALE)


def p(x: float, y: float) -> tuple[int, int]:
    return s(x), s(y)


def tile() -> Image.Image:
    return Image.new("RGBA", (CELL * SCALE, CELL * SCALE), (0, 0, 0, 0))


def line(draw: ImageDraw.ImageDraw, points, fill, width=2, joint="curve") -> None:
    draw.line([p(x, y) for x, y in points], fill=fill, width=max(1, s(width)), joint=joint)


def ellipse(draw: ImageDraw.ImageDraw, box, fill, outline=None, width=1) -> None:
    draw.ellipse(tuple(s(v) for v in box), fill=fill, outline=outline, width=max(1, s(width)))


def polygon(draw: ImageDraw.ImageDraw, points, fill, outline=None, width=1) -> None:
    draw.polygon([p(x, y) for x, y in points], fill=fill)
    if outline is not None:
        line(draw, [*points, points[0]], outline, width)


def arc(draw: ImageDraw.ImageDraw, box, start, end, fill, width=2) -> None:
    draw.arc(tuple(s(v) for v in box), start=start, end=end, fill=fill, width=max(1, s(width)))


def glow(base: Image.Image, points, color, width=2) -> None:
    layer = Image.new("RGBA", base.size, (0, 0, 0, 0))
    line(ImageDraw.Draw(layer), points, (*color[:3], 105), width * 3)
    base.alpha_composite(layer.filter(ImageFilter.GaussianBlur(s(5))))
    line(ImageDraw.Draw(base), points, color, width)


def frame_base(accent) -> tuple[Image.Image, ImageDraw.ImageDraw]:
    image = tile()
    draw = ImageDraw.Draw(image)
    ellipse(draw, (26, 26, 230, 230), (8, 18, 31, 90), (*accent[:3], 95), 2)
    arc(draw, (32, 32, 224, 224), 18, 160, (*accent[:3], 92), 2)
    arc(draw, (32, 32, 224, 224), 198, 340, (*accent[:3], 92), 2)
    return image, draw


def mara_rescue() -> Image.Image:
    image, draw = frame_base(CYAN)
    shield = [(128, 51), (190, 78), (181, 160), (128, 205), (75, 160), (66, 78)]
    polygon(draw, shield, STEEL_DARK, CYAN, 4)
    polygon(draw, [(128, 71), (168, 89), (162, 145), (128, 178), (94, 145), (88, 89)], STEEL, CYAN_SOFT, 2)
    line(draw, [(128, 94), (128, 151)], IVORY, 8)
    line(draw, [(101, 122), (155, 122)], IVORY, 8)
    glow(image, [(66, 78), (128, 51), (190, 78)], CYAN, 2)
    return image


def mara_rapid() -> Image.Image:
    image, draw = frame_base(AMBER)
    bolt = [(139, 37), (78, 126), (119, 126), (101, 219), (181, 111), (140, 111)]
    polygon(draw, bolt, AMBER, STEEL_DARK, 5)
    line(draw, [(47, 177), (82, 177)], AMBER, 4)
    line(draw, [(40, 151), (66, 151)], AMBER_SOFT, 3)
    line(draw, [(174, 69), (205, 69)], AMBER_SOFT, 3)
    return image


def ivo_rigger() -> Image.Image:
    image, draw = frame_base(CYAN)
    ellipse(draw, (76, 76, 180, 180), STEEL, CYAN, 4)
    ellipse(draw, (105, 105, 151, 151), STEEL_DARK, CYAN_SOFT, 3)
    for angle in range(0, 360, 45):
        r = math.radians(angle)
        x, y = 128 + math.cos(r) * 70, 128 + math.sin(r) * 70
        x2, y2 = 128 + math.cos(r) * 94, 128 + math.sin(r) * 94
        line(draw, [(x, y), (x2, y2)], CYAN, 7)
    line(draw, [(128, 128), (183, 70), (213, 70)], AMBER, 5)
    ellipse(draw, (205, 62, 221, 78), AMBER, STEEL_DARK, 2)
    return image


def ivo_smith() -> Image.Image:
    image, draw = frame_base(AMBER)
    ellipse(draw, (76, 76, 180, 180), STEEL_DARK, AMBER, 5)
    for angle in range(0, 360, 45):
        r = math.radians(angle)
        x, y = 128 + math.cos(r) * 75, 128 + math.sin(r) * 75
        ellipse(draw, (x - 12, y - 12, x + 12, y + 12), AMBER, STEEL_DARK, 2)
    ellipse(draw, (108, 108, 148, 148), STEEL, AMBER_SOFT, 3)
    line(draw, [(70, 190), (184, 76)], IVORY, 10)
    polygon(draw, [(174, 62), (205, 54), (221, 69), (207, 98), (184, 89)], AMBER, STEEL_DARK, 3)
    return image


def sena_deep() -> Image.Image:
    image, draw = frame_base(VIOLET)
    ellipse(draw, (51, 86, 205, 170), STEEL_DARK, VIOLET, 4)
    ellipse(draw, (91, 98, 165, 158), STEEL, VIOLET, 3)
    ellipse(draw, (113, 115, 143, 145), VIOLET, IVORY, 2)
    for radius, alpha in ((54, 80), (82, 55), (110, 35)):
        arc(draw, (128 - radius, 128 - radius, 128 + radius, 128 + radius), 205, 335, (*VIOLET[:3], alpha), 3)
    line(draw, [(128, 128), (207, 55)], CYAN, 3)
    ellipse(draw, (201, 49, 215, 63), CYAN, STEEL_DARK, 2)
    return image


def sena_ghost() -> Image.Image:
    image, draw = frame_base(MAGENTA)
    ghost = [(128, 52), (177, 92), (172, 164), (151, 188), (128, 168), (105, 188), (84, 164), (79, 92)]
    polygon(draw, ghost, STEEL_DARK, MAGENTA, 4)
    ellipse(draw, (101, 103, 119, 121), MAGENTA, IVORY, 2)
    ellipse(draw, (137, 103, 155, 121), MAGENTA, IVORY, 2)
    line(draw, [(103, 149), (128, 161), (153, 149)], MAGENTA, 4)
    for x in (50, 70, 186, 206):
        line(draw, [(x, 65), (x + (8 if x < 128 else -8), 49)], MAGENTA, 3)
    return image


def olan_lattice() -> Image.Image:
    image, draw = frame_base(GREEN)
    for x in (72, 128, 184):
        line(draw, [(x, 70), (x, 186)], GREEN, 3)
    for y in (72, 128, 184):
        line(draw, [(70, y), (186, y)], GREEN, 3)
    for x, y in ((72, 72), (128, 72), (184, 72), (72, 128), (128, 128), (184, 128), (72, 184), (128, 184), (184, 184)):
        ellipse(draw, (x - 9, y - 9, x + 9, y + 9), STEEL_DARK, GREEN, 3)
    polygon(draw, [(128, 84), (171, 128), (128, 172), (85, 128)], (54, 106, 78, 160), GREEN, 3)
    return image


def olan_decoder() -> Image.Image:
    image, draw = frame_base(GREEN)
    glyphs = [(76, 79, 180, 79), (76, 105, 154, 105), (76, 131, 180, 131), (76, 157, 136, 157)]
    for x1, y1, x2, y2 in glyphs:
        glow(image, [(x1, y1), (x2, y2)], GREEN, 3)
    polygon(draw, [(182, 73), (213, 103), (182, 133)], IVORY, STEEL_DARK, 3)
    line(draw, [(187, 153), (213, 183)], AMBER, 4)
    line(draw, [(213, 153), (187, 183)], AMBER, 4)
    return image


def main() -> None:
    builders = [mara_rescue, ivo_rigger, sena_deep, olan_lattice, mara_rapid, ivo_smith, sena_ghost, olan_decoder]
    source = Image.new("RGBA", (CELL * COLS * SCALE, CELL * ROWS * SCALE), (0, 0, 0, 0))
    for index, builder in enumerate(builders):
        x = (index % COLS) * CELL * SCALE
        y = (index // COLS) * CELL * SCALE
        source.alpha_composite(builder(), (x, y))
    SOURCE.parent.mkdir(parents=True, exist_ok=True)
    source.save(SOURCE, format="PNG")
    runtime = source.resize((CELL * COLS, CELL * ROWS), Image.Resampling.LANCZOS)
    OUT.parent.mkdir(parents=True, exist_ok=True)
    runtime.save(OUT, format="PNG", optimize=True)
    print(f"wrote source {SOURCE} ({source.width}x{source.height})")
    print(f"wrote runtime {OUT} ({runtime.width}x{runtime.height})")


if __name__ == "__main__":
    main()
