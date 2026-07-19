#!/usr/bin/env python3
"""Remove baked checkerboards and normalize a generated horizontal sprite strip."""

from __future__ import annotations

import argparse
from collections import deque
from pathlib import Path

from PIL import Image


def background_candidate(pixel: tuple[int, int, int], dark_background: bool) -> bool:
    low = min(pixel)
    high = max(pixel)
    if dark_background:
        return high <= 28
    return low >= 220 and high - low <= 18


def extract_alpha(image: Image.Image) -> Image.Image:
    rgb = image.convert("RGB")
    width, height = rgb.size
    pixels = rgb.load()
    corners = [
        pixels[0, 0],
        pixels[width - 1, 0],
        pixels[0, height - 1],
        pixels[width - 1, height - 1],
    ]
    dark_background = sum(sum(pixel) for pixel in corners) / 12 < 64
    visited = bytearray(width * height)
    frontier: deque[tuple[int, int]] = deque()

    def enqueue(x: int, y: int) -> None:
        index = y * width + x
        if not visited[index] and background_candidate(pixels[x, y], dark_background):
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

    rgba = rgb.convert("RGBA")
    alpha = Image.new("L", (width, height), 255)
    alpha.putdata([0 if value else 255 for value in visited])
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


def normalize(input_path: Path, output_path: Path, frames: int, frame_size: int) -> None:
    source = Image.open(input_path).convert("RGB")
    slot_width = source.width // frames
    if source.width % frames:
        raise ValueError(f"source width {source.width} is not divisible by {frames}")

    cleaned: list[Image.Image] = []
    bounds: list[tuple[int, int, int, int]] = []
    for index in range(frames):
        slot = source.crop((index * slot_width, 0, (index + 1) * slot_width, source.height))
        transparent = extract_alpha(slot)
        box = transparent.getchannel("A").getbbox()
        if box is None:
            raise ValueError(f"frame {index + 1} contains no foreground")
        cleaned.append(transparent)
        bounds.append(box)

    padding = max(8, frame_size // 16)
    maximum_width = max(right - left for left, _, right, _ in bounds)
    maximum_height = max(bottom - top for _, top, _, bottom in bounds)
    scale = min(
        (frame_size - padding * 2) / maximum_width,
        (frame_size - padding * 2) / maximum_height,
    )

    strip = Image.new("RGBA", (frame_size * frames, frame_size), (0, 0, 0, 0))
    for index, (frame, box) in enumerate(zip(cleaned, bounds, strict=True)):
        cropped = frame.crop(box)
        target = (
            max(1, round(cropped.width * scale)),
            max(1, round(cropped.height * scale)),
        )
        resized = cropped.resize(target, Image.Resampling.LANCZOS)
        x = index * frame_size + (frame_size - target[0]) // 2
        y = (frame_size - target[1]) // 2
        strip.alpha_composite(resized, (x, y))

    output_path.parent.mkdir(parents=True, exist_ok=True)
    strip.save(output_path, optimize=True)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--input", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--frames", type=int, default=6)
    parser.add_argument("--frame-size", type=int, default=256)
    args = parser.parse_args()
    normalize(args.input, args.output, args.frames, args.frame_size)


if __name__ == "__main__":
    main()
