#!/usr/bin/env python3
"""Generate Last Light's transparent top-down map-prop atlas.

The environment plates establish material and mood, while small props give
the player spatial landmarks for worker routes and tactical decisions.  This
generator keeps those props as independent, alpha-capable cells so they can be
placed over any of the seven sector plates without baking collision, fog, or
selection state into the image.  All geometry is deterministic local Pillow
work; no network or image-generation service is required.

Atlas layout (3 columns x 2 rows, 256 px runtime cells):

    cargo stack | tool bench | pipe cluster
    med locker  | relay node | Choir glyph

The high-resolution source is retained beside the script for art review and
the downsampled RGBA atlas is the file embedded by the game catalog.
"""

from __future__ import annotations

import math
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter


ROOT = Path(__file__).resolve().parents[3]
OUT = ROOT / "games/last-light/assets/map-props-atlas-v001.png"
SOURCE = ROOT / "tools/asset-sources/last-light/map-props-atlas-v001-source.png"

SCALE = 4
CELL = 256
COLS = 3
ROWS = 2
ATLAS_SIZE = (CELL * COLS, CELL * ROWS)

# Shared production-guide palette.  Props use restrained emissive accents so
# their silhouette remains readable before bloom and against light floor tiles.
STEEL_DARK = (17, 29, 39, 245)
STEEL = (38, 56, 72, 255)
STEEL_LIGHT = (78, 104, 120, 255)
IVORY = (215, 216, 204, 245)
CYAN = (57, 214, 208, 235)
CYAN_SOFT = (92, 239, 229, 150)
AMBER = (242, 169, 59, 235)
AMBER_SOFT = (255, 205, 104, 150)
MAGENTA = (242, 59, 147, 235)
MAGENTA_SOFT = (246, 103, 176, 145)


def scaled(value: float) -> int:
    return round(value * SCALE)


def point(value: tuple[float, float]) -> tuple[int, int]:
    return scaled(value[0]), scaled(value[1])


def tile() -> Image.Image:
    return Image.new("RGBA", (CELL * SCALE, CELL * SCALE), (0, 0, 0, 0))


def draw_line(
    draw: ImageDraw.ImageDraw,
    points: list[tuple[float, float]],
    fill: tuple[int, int, int, int],
    width: float = 1.0,
    joint: str = "curve",
) -> None:
    draw.line([point(p) for p in points], fill=fill, width=max(1, scaled(width)), joint=joint)


def draw_polygon(
    draw: ImageDraw.ImageDraw,
    points: list[tuple[float, float]],
    fill: tuple[int, int, int, int],
    outline: tuple[int, int, int, int] | None = None,
    width: float = 1.0,
) -> None:
    draw.polygon([point(p) for p in points], fill=fill)
    if outline is not None:
        draw_line(draw, [*points, points[0]], outline, width)


def draw_rect(
    draw: ImageDraw.ImageDraw,
    box: tuple[float, float, float, float],
    fill: tuple[int, int, int, int],
    outline: tuple[int, int, int, int] | None = None,
    width: float = 1.0,
) -> None:
    scaled_box = tuple(scaled(value) for value in box)
    draw.rectangle(scaled_box, fill=fill, outline=outline, width=max(1, scaled(width)))


def draw_round_rect(
    draw: ImageDraw.ImageDraw,
    box: tuple[float, float, float, float],
    radius: float,
    fill: tuple[int, int, int, int],
    outline: tuple[int, int, int, int] | None = None,
    width: float = 1.0,
) -> None:
    scaled_box = tuple(scaled(value) for value in box)
    draw.rounded_rectangle(
        scaled_box,
        radius=scaled(radius),
        fill=fill,
        outline=outline,
        width=max(1, scaled(width)),
    )


def draw_ellipse(
    draw: ImageDraw.ImageDraw,
    box: tuple[float, float, float, float],
    fill: tuple[int, int, int, int],
    outline: tuple[int, int, int, int] | None = None,
    width: float = 1.0,
) -> None:
    draw.ellipse(
        tuple(scaled(value) for value in box),
        fill=fill,
        outline=outline,
        width=max(1, scaled(width)),
    )


def draw_arc(
    draw: ImageDraw.ImageDraw,
    box: tuple[float, float, float, float],
    start: float,
    end: float,
    fill: tuple[int, int, int, int],
    width: float = 1.0,
) -> None:
    draw.arc(tuple(scaled(value) for value in box), start, end, fill=fill, width=max(1, scaled(width)))


def glow_line(base: Image.Image, points: list[tuple[float, float]], color: tuple[int, int, int], width: float) -> None:
    rgb = color[:3]
    glow = Image.new("RGBA", base.size, (0, 0, 0, 0))
    glow_draw = ImageDraw.Draw(glow)
    draw_line(glow_draw, points, (*rgb, 95), width * 3.5)
    base.alpha_composite(glow.filter(ImageFilter.GaussianBlur(scaled(4))))
    draw_line(ImageDraw.Draw(base), points, (*rgb, 220), width)


def rivets(draw: ImageDraw.ImageDraw, coords: list[tuple[float, float]], color=AMBER_SOFT) -> None:
    for x, y in coords:
        draw_ellipse(draw, (x - 2.5, y - 2.5, x + 2.5, y + 2.5), color)


def cargo_stack() -> Image.Image:
    """Three strapped salvage crates: the safe opening-route landmark."""

    image = tile()
    draw = ImageDraw.Draw(image)
    crates = [
        (42, 48, 133, 118, STEEL, AMBER),
        (121, 73, 214, 143, STEEL_LIGHT, AMBER_SOFT),
        (63, 126, 160, 205, STEEL_DARK, AMBER),
    ]
    for left, top, right, bottom, body, strap in crates:
        draw_round_rect(draw, (left, top, right, bottom), 8, body, STEEL_DARK, 4)
        draw_line(draw, [(left + 12, top + 13), (right - 12, top + 13)], STEEL_LIGHT, 2)
        draw_line(draw, [(left + 12, bottom - 13), (right - 12, bottom - 13)], STEEL_DARK, 2)
        draw_line(draw, [(left + 22, top + 5), (left + 22, bottom - 5)], strap, 4)
        draw_line(draw, [(right - 22, top + 5), (right - 22, bottom - 5)], strap, 4)
        rivets(draw, [(left + 10, top + 10), (right - 10, top + 10), (left + 10, bottom - 10), (right - 10, bottom - 10)])
    # A small cyan inventory seal gives players a consistent interactable cue.
    draw_ellipse(draw, (105, 104, 151, 150), (7, 19, 28, 235), CYAN, 3)
    draw_line(draw, [(116, 127), (140, 127)], CYAN_SOFT, 3)
    draw_line(draw, [(128, 115), (128, 139)], CYAN_SOFT, 3)
    return image


def tool_bench() -> Image.Image:
    """Engineer-facing bench with clamps, diagnostic panel, and tool arms."""

    image = tile()
    draw = ImageDraw.Draw(image)
    draw_round_rect(draw, (36, 70, 220, 186), 12, STEEL_DARK, STEEL_LIGHT, 5)
    draw_round_rect(draw, (50, 83, 207, 164), 7, STEEL, STEEL_DARK, 3)
    draw_rect(draw, (72, 99, 151, 145), (10, 28, 39, 255), CYAN, 3)
    draw_line(draw, [(85, 121), (137, 121)], CYAN_SOFT, 3)
    draw_line(draw, [(85, 130), (120, 130)], CYAN_SOFT, 2)
    draw_ellipse(draw, (178, 101, 195, 118), AMBER, STEEL_DARK, 2)
    draw_ellipse(draw, (178, 128, 195, 145), CYAN, STEEL_DARK, 2)
    # Tool arms are deliberately asymmetric: this keeps the Engineer's job
    # landmark distinct from a resource node or a power relay.
    draw_line(draw, [(55, 164), (31, 205), (62, 218)], STEEL_LIGHT, 8)
    draw_line(draw, [(204, 164), (229, 203), (198, 220)], AMBER, 7)
    draw_ellipse(draw, (23, 198, 42, 217), CYAN, STEEL_DARK, 3)
    draw_ellipse(draw, (219, 195, 239, 215), AMBER_SOFT, STEEL_DARK, 3)
    rivets(draw, [(59, 91), (199, 91), (59, 156), (199, 156)], CYAN_SOFT)
    return image


def pipe_cluster() -> Image.Image:
    """Conduit bundle used to break up corridors without hiding unit paths."""

    image = tile()
    draw = ImageDraw.Draw(image)
    paths = [
        ([(46, 67), (79, 67), (97, 91), (97, 182), (130, 210)], STEEL_LIGHT, CYAN),
        ([(75, 45), (112, 45), (133, 73), (133, 164), (174, 203)], STEEL, AMBER),
        ([(115, 36), (154, 36), (183, 63), (183, 145), (216, 176)], STEEL_LIGHT, CYAN),
    ]
    for points, metal, end_color in paths:
        draw_line(draw, points, STEEL_DARK, 18)
        draw_line(draw, points, metal, 11)
        draw_line(draw, points, (112, 139, 151, 150), 3)
        start = points[0]
        end = points[-1]
        draw_ellipse(draw, (start[0] - 10, start[1] - 10, start[0] + 10, start[1] + 10), end_color, STEEL_DARK, 3)
        draw_ellipse(draw, (end[0] - 10, end[1] - 10, end[0] + 10, end[1] + 10), end_color, STEEL_DARK, 3)
    draw_ellipse(draw, (117, 108, 157, 148), STEEL_DARK, STEEL_LIGHT, 3)
    draw_ellipse(draw, (126, 117, 148, 139), CYAN, STEEL_DARK, 2)
    return image


def med_locker() -> Image.Image:
    """Compact med locker that reads as a support waypoint at tactical scale."""

    image = tile()
    draw = ImageDraw.Draw(image)
    shell = [(65, 50), (191, 50), (218, 85), (206, 192), (176, 215), (79, 215), (49, 190), (38, 85)]
    draw_polygon(draw, shell, IVORY, STEEL_DARK, 5)
    inner = [(77, 73), (179, 73), (193, 92), (186, 174), (169, 191), (87, 191), (68, 173), (61, 91)]
    draw_polygon(draw, inner, STEEL, STEEL_LIGHT, 3)
    draw_rect(draw, (98, 86, 158, 172), (11, 29, 40, 245), CYAN_SOFT, 2)
    draw_rect(draw, (118, 95, 138, 163), IVORY)
    draw_rect(draw, (98, 119, 158, 140), IVORY)
    draw_ellipse(draw, (173, 90, 189, 106), AMBER, STEEL_DARK, 2)
    draw_line(draw, [(79, 60), (177, 60)], CYAN, 3)
    rivets(draw, [(57, 95), (199, 95), (59, 180), (197, 180)], CYAN_SOFT)
    return image


def relay_node() -> Image.Image:
    """Lantern power junction for map dressing near relay routes."""

    image = tile()
    draw = ImageDraw.Draw(image)
    center = (128, 128)
    for radius, alpha in ((96, 32), (78, 48), (58, 72)):
        draw_arc(draw, (center[0] - radius, center[1] - radius, center[0] + radius, center[1] + radius), 12, 165, (*CYAN[:3], alpha), 3)
        draw_arc(draw, (center[0] - radius, center[1] - radius, center[0] + radius, center[1] + radius), 192, 345, (*AMBER[:3], alpha), 3)
    spokes = [(128, 30, 128, 84), (128, 172, 128, 226), (30, 128, 84, 128), (172, 128, 226, 128)]
    for x1, y1, x2, y2 in spokes:
        draw_line(draw, [(x1, y1), (x2, y2)], STEEL_DARK, 13)
        glow_line(image, [(x1, y1), (x2, y2)], CYAN, 3)
    draw_polygon(draw, [(128, 65), (176, 101), (176, 155), (128, 191), (80, 155), (80, 101)], STEEL_DARK, CYAN, 4)
    draw_polygon(draw, [(128, 82), (158, 106), (158, 148), (128, 173), (98, 148), (98, 106)], STEEL, STEEL_LIGHT, 2)
    draw_ellipse(draw, (115, 115, 141, 141), AMBER, STEEL_DARK, 3)
    draw_ellipse(draw, (122, 122, 134, 134), CYAN_SOFT)
    return image


def choir_glyph() -> Image.Image:
    """Magenta radial burn that identifies a Choir-controlled route."""

    image = tile()
    draw = ImageDraw.Draw(image)
    center = (128, 128)
    for radius, alpha in ((103, 24), (82, 42), (62, 72)):
        draw_arc(draw, (center[0] - radius, center[1] - radius, center[0] + radius, center[1] + radius), 20, 160, (*MAGENTA[:3], alpha), 3)
        draw_arc(draw, (center[0] - radius, center[1] - radius, center[0] + radius, center[1] + radius), 200, 340, (*MAGENTA[:3], alpha), 3)
    for angle in range(0, 360, 60):
        radians = math.radians(angle)
        inner = (128 + math.cos(radians) * 43, 128 + math.sin(radians) * 43)
        outer = (128 + math.cos(radians) * 99, 128 + math.sin(radians) * 99)
        draw_line(draw, [inner, outer], MAGENTA_SOFT, 3)
    glyph = [(128, 47), (151, 96), (202, 112), (164, 139), (172, 193), (128, 165), (84, 193), (92, 139), (54, 112), (105, 96)]
    draw_polygon(draw, glyph, (38, 9, 31, 185), MAGENTA, 4)
    draw_polygon(draw, [(128, 83), (153, 128), (128, 177), (103, 128)], (63, 12, 50, 235), MAGENTA_SOFT, 3)
    draw_ellipse(draw, (118, 118, 138, 138), MAGENTA, (255, 198, 231, 220), 2)
    return image


def place(atlas: Image.Image, image: Image.Image, column: int, row: int) -> None:
    atlas.alpha_composite(image, (column * CELL * SCALE, row * CELL * SCALE))


def main() -> None:
    source = Image.new("RGBA", (ATLAS_SIZE[0] * SCALE, ATLAS_SIZE[1] * SCALE), (0, 0, 0, 0))
    place(source, cargo_stack(), 0, 0)
    place(source, tool_bench(), 1, 0)
    place(source, pipe_cluster(), 2, 0)
    place(source, med_locker(), 0, 1)
    place(source, relay_node(), 1, 1)
    place(source, choir_glyph(), 2, 1)

    SOURCE.parent.mkdir(parents=True, exist_ok=True)
    OUT.parent.mkdir(parents=True, exist_ok=True)
    source.save(SOURCE, format="PNG", optimize=False)
    runtime = source.resize(ATLAS_SIZE, Image.Resampling.LANCZOS)
    runtime.save(OUT, format="PNG", optimize=True)
    print(f"wrote source {SOURCE} ({source.width}x{source.height})")
    print(f"wrote runtime {OUT} ({runtime.width}x{runtime.height})")


if __name__ == "__main__":
    main()
