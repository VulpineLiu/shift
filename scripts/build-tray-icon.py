#!/usr/bin/env python3
"""Redraw src-tauri/icons/tray-template.png, the mark the system tray shows.

    scripts/build-tray-icon.py [--out PATH]

The tray is drawn by the shell, not by Shift, and at 16-18 px the app's colour
wheel is a blob: macOS tints a *template* image black or white to match the menu
bar, and the Windows notification area simply shrinks whatever it is given. So
this file is a flat single-tint mark whose only real channel is alpha — the same
shape as the radial ring the app itself draws, so the two read as one product.

Only Pillow is needed. The mark is drawn at 4x and averaged down so the wedge
edges and the cross arms stay smooth at every size the shell asks for.
"""

import argparse
import math
import pathlib

from PIL import Image, ImageDraw

SIZE = 88  # source edge in px; tray-icon renders the image 18 pt tall
SS = 4  # supersample on top of SIZE
CANVAS = SIZE * SS
CENTER = CANVAS / 2

OUTER = 38.0  # wedge ring, outer radius
INNER = 21.0  # wedge ring, inner radius
HUB = 12.6  # solid centre disc
GAP = 5.2  # degrees of air between wedges
PLUS_ARM = 7.8  # half-length of the cross cut out of the hub
PLUS_BAR = 2.8  # half-thickness of that cross
TINT = (128, 128, 128)  # unread by macOS, ignored except as a base for Windows


def ring_segment(radius_out, radius_in, start, stop):
    """Corners of a filled annular sector, outer arc forward then inner back."""
    steps = max(2, int(stop - start) + 1)
    points = [
        (CENTER + radius_out * math.cos(math.radians(a)), CENTER + radius_out * math.sin(math.radians(a)))
        for a in [start + (stop - start) * i / steps for i in range(steps + 1)]
    ]
    points += [
        (CENTER + radius_in * math.cos(math.radians(a)), CENTER + radius_in * math.sin(math.radians(a)))
        for a in [start + (stop - start) * i / steps for i in range(steps, -1, -1)]
    ]
    return points


def render_mask() -> Image.Image:
    mask = Image.new("L", (CANVAS, CANVAS), 0)
    draw = ImageDraw.Draw(mask)
    for wedge in range(6):
        start = -90 + wedge * 60 + GAP / 2
        stop = -90 + (wedge + 1) * 60 - GAP / 2
        draw.polygon(ring_segment(OUTER * SS, INNER * SS, start, stop), fill=255)
    draw.ellipse(
        [CENTER - HUB * SS, CENTER - HUB * SS, CENTER + HUB * SS, CENTER + HUB * SS], fill=255
    )
    width = int(PLUS_BAR * SS)
    draw.line(
        [CENTER - PLUS_ARM * SS, CENTER, CENTER + PLUS_ARM * SS, CENTER], fill=0, width=width
    )
    draw.line(
        [CENTER, CENTER - PLUS_ARM * SS, CENTER, CENTER + PLUS_ARM * SS], fill=0, width=width
    )
    return mask.resize((SIZE, SIZE), Image.LANCZOS)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--out",
        type=pathlib.Path,
        default=pathlib.Path(__file__).resolve().parent.parent
        / "src-tauri"
        / "icons"
        / "tray-template.png",
    )
    args = parser.parse_args()

    icon = Image.new("RGBA", (SIZE, SIZE), TINT + (0,))
    icon.putalpha(render_mask())
    args.out.parent.mkdir(parents=True, exist_ok=True)
    icon.save(args.out)
    print(f"wrote {args.out} ({SIZE}x{SIZE}, alpha-only monochrome mark)")


if __name__ == "__main__":
    main()
