#!/usr/bin/env python3
"""Reduce an opaque environment plate to a compact PNG or JPEG.

Generated environment plates can contain millions of near-duplicate colors.
An indexed palette works for graphic plates, while JPEG preserves continuous
material detail at a much lower byte cost. Sprite and UI assets should use
their own alpha-preserving pipeline instead of this utility.
"""

from __future__ import annotations

import argparse
from pathlib import Path

from PIL import Image


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Optimize an opaque environment plate.")
    parser.add_argument("--input", required=True, help="Source RGB/RGBA PNG.")
    parser.add_argument("--output", required=True, help="Output indexed-color PNG.")
    parser.add_argument(
        "--colors",
        type=int,
        default=256,
        help="Palette size, from 2 through 256. Default: 256.",
    )
    parser.add_argument(
        "--dither",
        choices=("none", "floyd-steinberg"),
        default="floyd-steinberg",
        help="Palette dithering mode. Default: floyd-steinberg.",
    )
    parser.add_argument(
        "--quality",
        type=int,
        default=70,
        help="JPEG quality, from 1 through 95. Ignored for PNG. Default: 70.",
    )
    parser.add_argument(
        "--width",
        type=int,
        help="Optional output width; height is derived from the source aspect ratio.",
    )
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    if not 2 <= args.colors <= 256:
        raise SystemExit("--colors must be between 2 and 256")
    if not 1 <= args.quality <= 95:
        raise SystemExit("--quality must be between 1 and 95")
    if args.width is not None and args.width <= 0:
        raise SystemExit("--width must be positive")

    source = Image.open(args.input).convert("RGB")
    if args.width is not None and args.width != source.width:
        output_height = max(1, round(source.height * args.width / source.width))
        source = source.resize((args.width, output_height), Image.Resampling.LANCZOS)
    output = Path(args.output)
    output.parent.mkdir(parents=True, exist_ok=True)
    if output.suffix.lower() in {".jpg", ".jpeg"}:
        source.save(
            output,
            format="JPEG",
            quality=args.quality,
            optimize=True,
            progressive=True,
            subsampling="4:2:0",
        )
        print(
            f"optimized {source.width}x{source.height} to JPEG quality "
            f"{args.quality} at {output}"
        )
        return
    if output.suffix.lower() != ".png":
        raise SystemExit("--output must end in .png, .jpg, or .jpeg")

    optimized = source.quantize(
        colors=args.colors,
        method=Image.Quantize.MEDIANCUT,
        dither=(
            Image.Dither.NONE
            if args.dither == "none"
            else Image.Dither.FLOYDSTEINBERG
        ),
    )

    optimized.save(output, optimize=True)
    print(
        f"optimized {source.width}x{source.height} to {args.colors} colors "
        f"with {args.dither} dithering at {output}"
    )


if __name__ == "__main__":
    main()
