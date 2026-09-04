#!/usr/bin/env python3
"""Pack normalized square sprite frames into one horizontal RGBA strip."""

from __future__ import annotations

import argparse
from pathlib import Path

from PIL import Image


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Pack numbered sprite frames into a strip.")
    parser.add_argument("--frames-dir", required=True, help="Directory containing NN.png frames.")
    parser.add_argument("--output", required=True, help="Output horizontal strip PNG.")
    parser.add_argument("--frames", type=int, required=True, help="Expected frame count.")
    parser.add_argument("--frame-size", type=int, required=True, help="Expected square frame size.")
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    if args.frames < 1 or args.frame_size < 1:
        raise SystemExit("--frames and --frame-size must be positive")

    frame_dir = Path(args.frames_dir)
    frames: list[Image.Image] = []
    for index in range(1, args.frames + 1):
        path = frame_dir / f"{index:02d}.png"
        if not path.is_file():
            raise SystemExit(f"missing frame: {path}")
        frame = Image.open(path).convert("RGBA")
        if frame.size != (args.frame_size, args.frame_size):
            raise SystemExit(
                f"{path} has size {frame.size}; expected {(args.frame_size, args.frame_size)}"
            )
        if frame.getchannel("A").getbbox() is None:
            raise SystemExit(f"{path} has no visible content")
        frames.append(frame)

    strip = Image.new("RGBA", (args.frames * args.frame_size, args.frame_size), (0, 0, 0, 0))
    for index, frame in enumerate(frames):
        strip.alpha_composite(frame, (index * args.frame_size, 0))

    output = Path(args.output)
    output.parent.mkdir(parents=True, exist_ok=True)
    strip.save(output)
    print(f"packed {args.frames} frames into {output} ({strip.width}x{strip.height})")


if __name__ == "__main__":
    main()
