#!/usr/bin/env python3
"""Generate the small transparent terrain-detail atlas used by Last Light.

The tactical map is intentionally rendered over authored environment plates.
This atlas supplies four low-alpha, top-down decals that can be layered without
covering units or objectives: a high-ground contour, a cover pocket, a thermal
fissure, and a resource beacon marker.  Keeping generation deterministic makes
the art reproducible in local asset validation and avoids a run-time dependency
on an image-generation service.
"""

from __future__ import annotations

import math
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter


ROOT = Path(__file__).resolve().parents[3]
OUT = ROOT / "games/last-light/assets/terrain-detail-atlas-v001.png"
SCALE = 4
CELL = 256
CANVAS = CELL * 2


def rgba_layer() -> Image.Image:
    return Image.new("RGBA", (CANVAS * SCALE, CANVAS * SCALE), (0, 0, 0, 0))


def xy(point: tuple[float, float]) -> tuple[int, int]:
    return (round(point[0] * SCALE), round(point[1] * SCALE))


def line(draw: ImageDraw.ImageDraw, points, fill, width=1, joint="curve") -> None:
    draw.line([xy(p) for p in points], fill=fill, width=max(1, round(width * SCALE)), joint=joint)


def polygon(draw: ImageDraw.ImageDraw, points, fill, outline=None, width=1) -> None:
    draw.polygon([xy(p) for p in points], fill=fill)
    if outline is not None:
        line(draw, [*points, points[0]], outline, width=width)


def arc(draw: ImageDraw.ImageDraw, box, start, end, fill, width=1) -> None:
    draw.arc(tuple(v * SCALE for v in box), start=start, end=end, fill=fill, width=max(1, round(width * SCALE)))


def place(base: Image.Image, tile: Image.Image, offset: tuple[int, int]) -> None:
    base.alpha_composite(tile, (offset[0] * SCALE, offset[1] * SCALE))


def glow_stroke(tile: Image.Image, points, color, width=2, blur=5) -> None:
    glow = Image.new("RGBA", tile.size, (0, 0, 0, 0))
    gdraw = ImageDraw.Draw(glow)
    line(gdraw, points, (*color, 120), width=width * 2.5)
    glow = glow.filter(ImageFilter.GaussianBlur(blur * SCALE))
    tile.alpha_composite(glow)
    draw = ImageDraw.Draw(tile)
    line(draw, points, (*color, 205), width=width)


def high_ground() -> Image.Image:
    tile = Image.new("RGBA", (CELL * SCALE, CELL * SCALE), (0, 0, 0, 0))
    draw = ImageDraw.Draw(tile)
    center = (128, 128)
    for radius, alpha in ((96, 18), (84, 26), (72, 38)):
        box = (center[0] - radius, center[1] - radius * 0.62, center[0] + radius, center[1] + radius * 0.62)
        arc(draw, box, 198, 342, (66, 213, 223, alpha), width=3)
    contour = [(36, 154), (57, 75), (120, 48), (202, 72), (220, 144), (176, 192), (91, 196)]
    polygon(draw, contour, (15, 144, 169, 30), outline=(73, 235, 232, 128), width=2)
    inner = [(62, 145), (76, 91), (123, 73), (180, 90), (192, 136), (161, 166), (101, 172)]
    polygon(draw, inner, (24, 193, 199, 22), outline=(112, 255, 245, 116), width=1)
    for x in range(70, 194, 24):
        line(draw, [(x, 165), (x + 16, 102)], (110, 252, 242, 118), width=1)
    for x, y in ((93, 122), (128, 100), (164, 128)):
        polygon(draw, [(x, y - 9), (x + 8, y + 5), (x, y + 1), (x - 8, y + 5)], (164, 255, 248, 170))
    return tile


def cover_pocket() -> Image.Image:
    tile = Image.new("RGBA", (CELL * SCALE, CELL * SCALE), (0, 0, 0, 0))
    draw = ImageDraw.Draw(tile)
    for radius, alpha in ((102, 16), (84, 25), (66, 38)):
        box = (128 - radius, 128 - radius, 128 + radius, 128 + radius)
        arc(draw, box, 32, 148, (161, 103, 226, alpha), width=4)
        arc(draw, box, 212, 328, (77, 183, 239, alpha), width=4)
    shield = [(47, 91), (79, 58), (128, 45), (177, 58), (209, 91), (197, 149), (159, 190), (128, 207), (97, 190), (59, 149)]
    polygon(draw, shield, (86, 65, 157, 26), outline=(182, 133, 248, 130), width=2)
    for angle in range(0, 360, 45):
        r = math.radians(angle)
        x = 128 + math.cos(r) * 78
        y = 128 + math.sin(r) * 58
        line(draw, [(128 + math.cos(r) * 63, 128 + math.sin(r) * 47), (x, y)], (122, 205, 247, 106), width=2)
    polygon(draw, [(128, 78), (140, 101), (128, 95), (116, 101)], (194, 158, 255, 170))
    return tile


def thermal_fissure() -> Image.Image:
    tile = Image.new("RGBA", (CELL * SCALE, CELL * SCALE), (0, 0, 0, 0))
    draw = ImageDraw.Draw(tile)
    main = [(33, 191), (73, 161), (82, 133), (116, 114), (130, 76), (165, 58), (218, 29)]
    glow_stroke(tile, main, (248, 144, 47), width=3, blur=4)
    branches = [
        [(76, 160), (50, 145), (30, 147)],
        [(82, 133), (105, 151), (119, 184)],
        [(116, 114), (145, 126), (177, 121)],
        [(130, 76), (112, 54), (105, 29)],
        [(165, 58), (183, 84), (214, 94)],
    ]
    for branch in branches:
        glow_stroke(tile, branch, (230, 86, 42), width=2, blur=3)
    for x, y in ((74, 166), (116, 112), (166, 57), (215, 29)):
        draw.ellipse((int((x - 5) * SCALE), int((y - 5) * SCALE), int((x + 5) * SCALE), int((y + 5) * SCALE)), fill=(255, 202, 91, 178))
    line(draw, [(40, 213), (216, 18)], (255, 185, 72, 66), width=1)
    return tile


def resource_beacon() -> Image.Image:
    tile = Image.new("RGBA", (CELL * SCALE, CELL * SCALE), (0, 0, 0, 0))
    draw = ImageDraw.Draw(tile)
    for radius, alpha in ((104, 18), (84, 30), (62, 52)):
        box = (128 - radius, 128 - radius, 128 + radius, 128 + radius)
        arc(draw, box, 8, 172, (56, 228, 227, alpha), width=3)
        arc(draw, box, 188, 352, (146, 88, 255, alpha), width=3)
    line(draw, [(128, 34), (128, 222)], (107, 255, 241, 70), width=1)
    line(draw, [(34, 128), (222, 128)], (176, 106, 255, 70), width=1)
    beacon = [(128, 48), (164, 112), (146, 105), (146, 166), (110, 166), (110, 105), (92, 112)]
    polygon(draw, beacon, (48, 206, 205, 55), outline=(122, 255, 235, 170), width=2)
    polygon(draw, [(128, 73), (145, 111), (128, 101), (111, 111)], (178, 255, 244, 194))
    draw.ellipse((118 * SCALE, 118 * SCALE, 138 * SCALE, 138 * SCALE), fill=(228, 255, 249, 225))
    return tile


def main() -> None:
    atlas = Image.new("RGBA", (CANVAS * SCALE, CANVAS * SCALE), (0, 0, 0, 0))
    place(atlas, high_ground(), (0, 0))
    place(atlas, cover_pocket(), (CELL, 0))
    place(atlas, thermal_fissure(), (0, CELL))
    place(atlas, resource_beacon(), (CELL, CELL))
    atlas = atlas.resize((CANVAS, CANVAS), Image.Resampling.LANCZOS)
    OUT.parent.mkdir(parents=True, exist_ok=True)
    atlas.save(OUT, format="PNG", optimize=True)
    print(f"wrote {OUT} ({atlas.width}x{atlas.height})")


if __name__ == "__main__":
    main()
