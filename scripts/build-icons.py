#!/usr/bin/env python3
"""Draw every Shift icon from one mark.

    scripts/build-icons.py              # tray PNG + assets/icon.svg
    scripts/build-icons.py --tauri      # ... then the full platform set

Both icons are the same drawing — a six-wedge ring around a hub with a plus
punched out of it, which is the radial ring the app itself paints — and they
differ only in what is allowed to be on screen behind it. The shell tints the
tray mark, so that file carries no colour at all: its only real channel is
alpha, because macOS renders a *template* image black or white to match the
menu bar and the Windows notification area simply shrinks whatever it is
given. At 16-18 px the old colour wheel was a blob, which is what "use a
black/white system icon" was about. The app icon is the same mark with its
colour turned on, because a Dock tile is not tinted by anything and needs a
ground of its own; it is flat and warm, and deliberately not a second
gradient, since the layered rainbow treatment is what made the two icons look
like they belonged to different products.

The geometry lives once, in the constants below. The tray file is rasterised
here; the app icon is written as an SVG master that `tauri icon` turns into
the .icns / .ico / Windows tiles / android / ios set, which is worth using
over hand-rolling: its icon.icns and icon.ico carry the same size ladders the
repository already ships, verified against the committed files.

Only Pillow is needed for the default run.
"""

import argparse
import math
import pathlib
import subprocess
import sys

from PIL import Image, ImageDraw

# --- the mark, in units where the wedge ring's outer radius is 38 -----------
# These are the numbers the ring is specified in, not a size: each consumer
# scales them. Changing GAP or PLUS_BAR changes both icons together, which is
# the whole point of this file.
OUTER = 38.0  # wedge ring, outer radius
INNER = 21.0  # wedge ring, inner radius
HUB = 12.6  # solid centre disc the plus is cut out of
GAP = 5.2  # degrees of air between wedges
PLUS_ARM = 7.8  # half-length of the cross cut out of the hub
PLUS_BAR = 2.8  # half-thickness of that cross
WHEEL = 2 * OUTER  # the mark's own diameter in these units

# --- tray ------------------------------------------------------------------
TRAY_EDGE = 88  # tray-icon renders the image 18 pt tall, so this is > 4x
TRAY_SS = 4  # supersample on top of TRAY_EDGE for smooth wedge edges
TRAY_TINT = (128, 128, 128)  # unread by macOS; only a base for Windows

# --- app icon --------------------------------------------------------------
MASTER = 1024  # the master canvas `tauri icon` downsamples from
TILE = 820  # tile edge; the committed icon.png is 410 of 512, i.e. 0.801
TILE_CORNER = TILE * 228 / 1024  # same 0.222 corner ratio as the committed set
WHEEL_R = 326  # wheel radius in master units
WEDGE_INK = "#FFB25C"  # the warm amber the old master already carried
HUB_INK = "#F04F24"  # the accent every button in the app is painted with
TILE_INK = "#37221A"  # the espresso tile the plus is punched out of

ROOT = pathlib.Path(__file__).resolve().parent.parent


def ring_segment(center, radius_out, radius_in, start, stop):
    """Corners of a filled annular sector, outer arc forward then inner back."""
    steps = max(2, int(stop - start) + 1)
    arc = lambda radius, index: (
        center + radius * math.cos(math.radians(start + (stop - start) * index / steps)),
        center + radius * math.sin(math.radians(start + (stop - start) * index / steps)),
    )
    points = [arc(radius_out, i) for i in range(steps + 1)]
    points += [arc(radius_in, i) for i in range(steps, -1, -1)]
    return points


def punch_cross(draw, center, scale):
    """Clear the plus out of whatever has been drawn, leaving the ground to show."""
    width = int(PLUS_BAR * scale)
    draw.line(
        [center - PLUS_ARM * scale, center, center + PLUS_ARM * scale, center],
        fill=0,
        width=width,
    )
    draw.line(
        [center, center - PLUS_ARM * scale, center, center + PLUS_ARM * scale],
        fill=0,
        width=width,
    )


def render_tray_mask():
    """The mark as an alpha mask at TRAY_EDGE, drawn at TRAY_SS for one smooth pass."""
    canvas = TRAY_EDGE * TRAY_SS
    center = canvas / 2
    mask = Image.new("L", (canvas, canvas), 0)
    draw = ImageDraw.Draw(mask)
    for wedge in range(6):
        start = -90 + wedge * 60 + GAP / 2
        stop = -90 + (wedge + 1) * 60 - GAP / 2
        draw.polygon(
            ring_segment(center, OUTER * TRAY_SS, INNER * TRAY_SS, start, stop), fill=255
        )
    hub = HUB * TRAY_SS
    draw.ellipse([center - hub, center - hub, center + hub, center + hub], fill=255)
    punch_cross(draw, center, TRAY_SS)
    return mask.resize((TRAY_EDGE, TRAY_EDGE), Image.LANCZOS)


def _shown(path: pathlib.Path) -> str:
    # Repo-relative when it is inside the repo; `--tray-out` may point anywhere.
    path = path.resolve()
    try:
        return str(path.relative_to(ROOT))
    except ValueError:
        return str(path)


def build_tray(path: pathlib.Path) -> None:
    icon = Image.new("RGBA", (TRAY_EDGE, TRAY_EDGE), TRAY_TINT + (0,))
    icon.putalpha(render_tray_mask())
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(_png_bytes(icon))
    print(f"wrote {_shown(path)} ({TRAY_EDGE}x{TRAY_EDGE}, alpha-only monochrome mark)")


def _png_bytes(image: Image.Image) -> bytes:
    import io

    buffer = io.BytesIO()
    image.save(buffer, format="PNG")
    return buffer.getvalue()


def build_master_svg() -> str:
    """The app icon: the tray mark, with its colour turned on.

    What makes the two icons the same drawing is the geometry and the rule
    about how it is filled — six wedges, the same angular gaps, the same ring
    and hub radii, the plus punched out of the hub as negative space, and flat
    solid fills throughout. Colour is not the thing they have to agree about,
    and the tray is the one that cannot have any: macOS tints a template image
    and the Windows notification area just shrinks what it is given. A Dock
    tile sits on the user's wallpaper with nothing behind it, so it needs a
    ground of its own and gets to be warm.

    Three flat colours and nothing else. No gradient, no drop shadow, no
    radial shine and no per-wedge pastel tint, because that layered treatment
    is a different visual language and is exactly why the old rainbow tile and
    the crisp new mark read as two unrelated products. The hues are the app's
    own rather than invented for the icon: `#FFB25C` is the tone the previous
    master already carried and `#F04F24` is the accent every button in the
    interface is painted with, and keeping the darker warm disc inside the
    lighter ring preserves which end of the mark is which, so the Dock does not
    invert on upgrade.

    The plus is drawn in the tile's own colour instead of cut through to
    transparency: on a Dock tile the two read identically, and a real hole
    would be filled in by the platform wherever `tauri icon` composes the iOS
    and Android layers.

    The wheel is sized to 652 of the 820 tile because that is the ratio the
    icon this replaces was drawn at (r=326 in an 820 tile), so the new mark
    holds the same optical weight in the Dock rather than arriving larger.
    """
    scale = WHEEL_R / OUTER
    center = MASTER / 2
    offset = (MASTER - TILE) / 2
    bar = PLUS_BAR * scale
    arm = PLUS_ARM * scale
    wedges = []
    for wedge in range(6):
        start = -90 + wedge * 60 + GAP / 2
        stop = -90 + (wedge + 1) * 60 - GAP / 2
        outer_start = _point(center, OUTER * scale, start)
        outer_end = _point(center, OUTER * scale, stop)
        inner_start = _point(center, INNER * scale, stop)
        inner_end = _point(center, INNER * scale, start)
        wedges.append(
            f'    <path d="M{outer_start} A{OUTER * scale:.2f} {OUTER * scale:.2f} 0 0 1 '
            f'{outer_end} L{inner_start} A{INNER * scale:.2f} {INNER * scale:.2f} 0 0 0 '
            f'{inner_end} Z"/>'
        )
    return (
        f'<svg width="{MASTER}" height="{MASTER}" viewBox="0 0 {MASTER} {MASTER}" '
        'fill="none" xmlns="http://www.w3.org/2000/svg">\n'
        f"  <rect x=\"{offset:.0f}\" y=\"{offset:.0f}\" width=\"{TILE}\" height=\"{TILE}\" "
        f'rx="{TILE_CORNER:.2f}" fill="{TILE_INK}"/>\n'
        f'  <g fill="{WEDGE_INK}">\n' + "\n".join(wedges) + "\n  </g>\n"
        f'  <circle cx="{center:.0f}" cy="{center:.0f}" r="{HUB * scale:.2f}" fill="{HUB_INK}"/>\n'
        f'  <path d="M{center - arm:.2f} {center:.0f}H{center + arm:.2f}'
        f'M{center:.0f} {center - arm:.2f}V{center + arm:.2f}" '
        f'stroke="{TILE_INK}" stroke-width="{bar:.2f}"/>\n'
        "</svg>\n"
    )


def _point(center, radius, angle):
    return (
        f"{center + radius * math.cos(math.radians(angle)):.2f} "
        f"{center + radius * math.sin(math.radians(angle)):.2f}"
    )


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--tray-out",
        type=pathlib.Path,
        default=ROOT / "src-tauri" / "icons" / "tray-template.png",
    )
    parser.add_argument("--master-out", type=pathlib.Path, default=ROOT / "assets" / "icon.svg")
    parser.add_argument(
        "--tauri",
        action="store_true",
        help="also run `tauri icon` on the master to regenerate the platform set",
    )
    args = parser.parse_args()

    build_tray(args.tray_out)
    args.master_out.parent.mkdir(parents=True, exist_ok=True)
    args.master_out.write_text(build_master_svg(), encoding="utf-8")
    print(f"wrote {_shown(args.master_out)} (flat three-colour master)")

    if args.tauri:
        # Full-bleed would be wrong: `tauri icon` scales the master rather than
        # padding it, so the tile has to sit inside the canvas the way it does
        # in the committed set. That is why the rect above is inset.
        subprocess.run(
            ["pnpm", "tauri", "icon", str(args.master_out)], check=True, cwd=ROOT
        )
        # The brand mark in the window title bars is the same file at 26 px, so
        # it is a copy of the icon rather than a second drawing to keep in step.
        brand = ROOT / "public" / "shift-icon.png"
        brand.write_bytes((ROOT / "src-tauri" / "icons" / "icon.png").read_bytes())
        print(f"wrote {_shown(brand)} (same mark, shown at 26 px in the title bar)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
