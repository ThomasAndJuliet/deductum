#!/usr/bin/env python3
"""Regenerate packaging/macos/Deductum.icns from res/dev.twright.Deductum.svg.

Needs `rsvg-convert` (librsvg) to rasterise the SVG. The ICNS container is written with
the standard library: PNG entries ic07-ic10 (128 to 1024 px) and the @2x types ic11-ic14.
The result is committed, so packaging on a Mac does not need rsvg-convert."""
from __future__ import annotations

import argparse
from pathlib import Path
import struct
import subprocess

ROOT = Path(__file__).resolve().parents[1]
# ICNS type -> pixel size. ic11-ic14 are the @2x images of 16, 32, 128 and 256 points.
ENTRIES = (("ic07", 128), ("ic08", 256), ("ic09", 512), ("ic10", 1024),
           ("ic11", 32), ("ic12", 64), ("ic13", 256), ("ic14", 512))
PNG_MAGIC = b"\x89PNG\r\n\x1a\n"


def rasterise(svg: Path, size: int) -> bytes:
    png = subprocess.run(["rsvg-convert", "--width", str(size), "--height", str(size), "--format", "png", str(svg)],
                         check=True, capture_output=True).stdout
    if not png.startswith(PNG_MAGIC):
        raise ValueError(f"rsvg-convert did not produce a PNG at {size} px")
    return png


def icns(images: dict[int, bytes]) -> bytes:
    """Each entry is its type, a big-endian length that includes the 8-byte header, and the PNG."""
    body = b"".join(kind.encode("ascii") + struct.pack(">I", 8 + len(images[size])) + images[size]
                    for kind, size in ENTRIES)
    return b"icns" + struct.pack(">I", 8 + len(body)) + body


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--svg", type=Path, default=ROOT / "res/dev.twright.Deductum.svg")
    parser.add_argument("--output", type=Path, default=ROOT / "packaging/macos/Deductum.icns")
    args = parser.parse_args()
    images = {size: rasterise(args.svg, size) for size in sorted({size for _, size in ENTRIES})}
    args.output.write_bytes(icns(images))
    print(f"wrote {args.output}")


if __name__ == "__main__":
    main()
