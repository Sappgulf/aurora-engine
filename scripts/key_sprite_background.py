#!/usr/bin/env python3
"""Convert a baked neutral checkerboard around a generated sprite to alpha.

Image-generation previews sometimes return a checkerboard as RGB pixels even
when transparency was requested.  This utility removes only checkerboard
pixels connected to the image edge, keeping neutral highlights inside the
sprite intact.  It is intentionally conservative: the generated image still
needs the normal sprite-strip validation and preview steps afterward.
"""

from __future__ import annotations

import argparse
from collections import deque
from pathlib import Path

from PIL import Image


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Key an edge-connected neutral checkerboard to transparency."
    )
    parser.add_argument("--input", required=True, help="RGB/RGBA source image.")
    parser.add_argument("--output", required=True, help="RGBA output PNG.")
    parser.add_argument(
        "--min-channel",
        type=int,
        default=210,
        help="Minimum channel value for a candidate background pixel.",
    )
    parser.add_argument(
        "--max-spread",
        type=int,
        default=12,
        help="Maximum RGB channel spread for a candidate background pixel.",
    )
    return parser.parse_args()


def is_candidate(pixel: tuple[int, int, int, int], min_channel: int, max_spread: int) -> bool:
    red, green, blue, _ = pixel
    return min(red, green, blue) >= min_channel and max(pixel[:3]) - min(pixel[:3]) <= max_spread


def edge_connected_background(
    image: Image.Image, min_channel: int, max_spread: int
) -> set[tuple[int, int]]:
    width, height = image.size
    pixels = image.load()
    queue: deque[tuple[int, int]] = deque()
    visited: set[tuple[int, int]] = set()

    def enqueue(x: int, y: int) -> None:
        point = (x, y)
        if point in visited or not is_candidate(pixels[x, y], min_channel, max_spread):
            return
        visited.add(point)
        queue.append(point)

    for x in range(width):
        enqueue(x, 0)
        enqueue(x, height - 1)
    for y in range(height):
        enqueue(0, y)
        enqueue(width - 1, y)

    while queue:
        x, y = queue.popleft()
        for next_x, next_y in (
            (x - 1, y),
            (x + 1, y),
            (x, y - 1),
            (x, y + 1),
        ):
            if 0 <= next_x < width and 0 <= next_y < height:
                enqueue(next_x, next_y)
    return visited


def main() -> None:
    args = parse_args()
    if not 0 <= args.min_channel <= 255:
        raise SystemExit("--min-channel must be between 0 and 255")
    if not 0 <= args.max_spread <= 255:
        raise SystemExit("--max-spread must be between 0 and 255")

    image = Image.open(args.input).convert("RGBA")
    background = edge_connected_background(image, args.min_channel, args.max_spread)
    pixels = image.load()
    for x, y in background:
        red, green, blue, _ = pixels[x, y]
        pixels[x, y] = (red, green, blue, 0)

    output = Path(args.output)
    output.parent.mkdir(parents=True, exist_ok=True)
    image.save(output)
    print(f"removed {len(background)} edge-connected background pixels")


if __name__ == "__main__":
    main()
