#!/usr/bin/env python3
"""Remove baked backgrounds and normalize generated sprite strips or grids."""

from __future__ import annotations

import argparse
from collections import deque
from pathlib import Path

from PIL import Image


def background_candidate(
    pixel: tuple[int, int, int], dark_background: bool, neutral_matte: bool
) -> bool:
    low = min(pixel)
    high = max(pixel)
    if dark_background:
        return high <= 28
    if neutral_matte:
        # Generated checkerboards often use a darker neutral tile around
        # 205–220 and a lighter tile around 235–245. Treat both as matte only
        # when the source corners already prove the canvas is grayscale.
        return low >= 185 and high - low <= 18
    return low >= 220 and high - low <= 18


def extract_alpha(image: Image.Image) -> Image.Image:
    source = image.convert("RGBA")
    rgb = source.convert("RGB")
    width, height = rgb.size
    pixels = rgb.load()
    corners = [
        pixels[0, 0],
        pixels[width - 1, 0],
        pixels[0, height - 1],
        pixels[width - 1, height - 1],
    ]
    dark_background = sum(sum(pixel) for pixel in corners) / 12 < 64
    neutral_matte = (
        not dark_background
        and all(max(pixel) - min(pixel) <= 8 for pixel in corners)
        and min(min(pixel) for pixel in corners) >= 185
    )
    source_alpha = source.getchannel("A")
    alpha_pixels = source_alpha.load()
    visited = bytearray(width * height)
    frontier: deque[tuple[int, int]] = deque()

    def enqueue(x: int, y: int) -> None:
        index = y * width + x
        removable = alpha_pixels[x, y] == 0 or background_candidate(
            pixels[x, y], dark_background, neutral_matte
        )
        if not visited[index] and removable:
            visited[index] = 1
            frontier.append((x, y))

    for x in range(width):
        enqueue(x, 0)
        enqueue(x, height - 1)
    for y in range(height):
        enqueue(0, y)
        enqueue(width - 1, y)

    while frontier:
        x, y = frontier.popleft()
        if x > 0:
            enqueue(x - 1, y)
        if x + 1 < width:
            enqueue(x + 1, y)
        if y > 0:
            enqueue(x, y - 1)
        if y + 1 < height:
            enqueue(x, y + 1)

    rgba = source.copy()
    alpha = source_alpha.copy()
    output_alpha = alpha.load()
    for y in range(height):
        for x in range(width):
            if visited[y * width + x]:
                output_alpha[x, y] = 0
    rgba.putalpha(alpha)
    remove_small_islands(rgba)
    return rgba


def remove_small_islands(image: Image.Image) -> None:
    alpha = image.getchannel("A")
    width, height = alpha.size
    pixels = alpha.load()
    visited = bytearray(width * height)
    components: list[list[tuple[int, int]]] = []
    for start_y in range(height):
        for start_x in range(width):
            start = start_y * width + start_x
            if visited[start] or pixels[start_x, start_y] == 0:
                continue
            visited[start] = 1
            frontier = deque([(start_x, start_y)])
            component: list[tuple[int, int]] = []
            while frontier:
                x, y = frontier.popleft()
                component.append((x, y))
                for neighbor_x, neighbor_y in (
                    (x - 1, y),
                    (x + 1, y),
                    (x, y - 1),
                    (x, y + 1),
                ):
                    if not (0 <= neighbor_x < width and 0 <= neighbor_y < height):
                        continue
                    index = neighbor_y * width + neighbor_x
                    if visited[index] or pixels[neighbor_x, neighbor_y] == 0:
                        continue
                    visited[index] = 1
                    frontier.append((neighbor_x, neighbor_y))
            components.append(component)
    if not components:
        return
    largest = max(len(component) for component in components)
    cutoff = max(8, round(largest * 0.05))
    for component in components:
        if len(component) < cutoff:
            for x, y in component:
                pixels[x, y] = 0
    image.putalpha(alpha)


def normalize_grid(
    input_path: Path,
    output_path: Path,
    columns: int,
    rows: int,
    frame_size: int,
    row_bounds: list[tuple[int, int]] | None = None,
) -> None:
    source = Image.open(input_path).convert("RGBA")
    if source.width % columns:
        raise ValueError(f"source width {source.width} is not divisible by {columns}")
    if row_bounds is None and source.height % rows:
        raise ValueError(f"source height {source.height} is not divisible by {rows}")
    if row_bounds is not None and len(row_bounds) != rows:
        raise ValueError(f"expected {rows} row bounds, received {len(row_bounds)}")
    slot_width = source.width // columns
    slot_height = source.height // rows

    padding = max(8, frame_size // 16)
    atlas = Image.new(
        "RGBA", (frame_size * columns, frame_size * rows), (0, 0, 0, 0)
    )
    for row in range(rows):
        row_top, row_bottom = (
            row_bounds[row]
            if row_bounds is not None
            else (row * slot_height, (row + 1) * slot_height)
        )
        if not (0 <= row_top < row_bottom <= source.height):
            raise ValueError(f"invalid row bounds {row_top}:{row_bottom}")
        cleaned: list[Image.Image] = []
        bounds: list[tuple[int, int, int, int]] = []
        for column in range(columns):
            slot = source.crop(
                (
                    column * slot_width,
                    row_top,
                    (column + 1) * slot_width,
                    row_bottom,
                )
            )
            transparent = extract_alpha(slot)
            box = transparent.getchannel("A").getbbox()
            if box is None:
                raise ValueError(f"cell ({column + 1}, {row + 1}) contains no foreground")
            cleaned.append(transparent)
            bounds.append(box)

        maximum_width = max(right - left for left, _, right, _ in bounds)
        maximum_height = max(bottom - top for _, top, _, bottom in bounds)
        scale = min(
            (frame_size - padding * 2) / maximum_width,
            (frame_size - padding * 2) / maximum_height,
        )

        for column, (frame, box) in enumerate(zip(cleaned, bounds, strict=True)):
            cropped = frame.crop(box)
            target = (
                max(1, round(cropped.width * scale)),
                max(1, round(cropped.height * scale)),
            )
            resized = cropped.resize(target, Image.Resampling.LANCZOS)
            x = column * frame_size + (frame_size - target[0]) // 2
            y = row * frame_size + (frame_size - target[1]) // 2
            atlas.alpha_composite(resized, (x, y))

    output_path.parent.mkdir(parents=True, exist_ok=True)
    atlas.save(output_path, optimize=True)


def normalize(input_path: Path, output_path: Path, frames: int, frame_size: int) -> None:
    normalize_grid(input_path, output_path, frames, 1, frame_size)


def parse_row_bounds(value: str) -> list[tuple[int, int]]:
    try:
        return [tuple(map(int, band.split(":"))) for band in value.split(",")]
    except ValueError as error:
        raise argparse.ArgumentTypeError(
            "row bounds must look like 40:330,350:570"
        ) from error


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--input", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--frames", type=int, default=6)
    parser.add_argument("--columns", type=int)
    parser.add_argument("--rows", type=int, default=1)
    parser.add_argument("--row-bounds", type=parse_row_bounds)
    parser.add_argument("--frame-size", type=int, default=256)
    args = parser.parse_args()
    normalize_grid(
        args.input,
        args.output,
        args.columns or args.frames,
        args.rows,
        args.frame_size,
        args.row_bounds,
    )


if __name__ == "__main__":
    main()
