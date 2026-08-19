#!/usr/bin/python3
"""Convert axis-aligned pixel-art SVG to the badge's [u32; 512] bitmap format.

s-cam.svg is 92x92 with every path coordinate on a 4px grid, i.e. pixel art at 23x23. That
makes rasterising unnecessary and undesirable: sampling the grid directly reproduces the
artwork exactly, where a rasteriser would antialias edges that must stay hard on a 1bpp panel.

The panel is an EastRising ER-OLED1.12-1W: a 1.12" 128x128 white OLED on an SH1107
controller, per ux.kicad_sch in bunnie/dc34-core-hw. 128x128 is the full active area, not a
window onto something larger - verified on hardware by drawing a 1px border on the outermost
framebuffer pixels and confirming four clean edges. The dark surround is the module's glass,
which is not addressable, and the active area is not centred within it. So an image centred
in pixels can still look slightly off-centre in the module, and no software change fixes that.

At 4x the 23x23 art lands as 92x92 with exact 18px margins. 5x gives 115x115 with 6/7
margins - larger, but one pixel off, since 13 leftover pixels cannot split evenly.

Output matches pngtorust.py byte for byte in convention:
  - the image is flipped left-right
  - bits are inverted: 1 means black, 0 means white
  - bits pack MSB-first within each u32
  - words are emitted reversed within each group of four

Usage: svgtorust.py <in.svg> <out.rs> [scale]
"""
import os
import re
import sys
from functools import reduce
from math import gcd

FB = 128  # framebuffer is 128x128


def paths(svg):
    """Parse M/H/V/Z absolute path data into closed polygons."""
    out = []
    for d in re.findall(r'\sd="([^"]+)"', svg):
        pts, x, y = [], 0.0, 0.0
        for cmd, arg in re.findall(r'([MHVZmhvz])([-\d.\s]*)', d):
            n = [float(v) for v in arg.replace(",", " ").split()]
            if cmd == "M":
                x, y = n[0], n[1]
                pts.append((x, y))
            elif cmd == "H":
                x = n[0]
                pts.append((x, y))
            elif cmd == "V":
                y = n[0]
                pts.append((x, y))
        if len(pts) >= 3:
            out.append(pts)
    return out


def inside(px, py, poly):
    """Even-odd point-in-polygon."""
    hit = False
    n = len(poly)
    for i in range(n):
        x0, y0 = poly[i]
        x1, y1 = poly[(i + 1) % n]
        if (y0 > py) != (y1 > py):
            xint = x0 + (py - y0) * (x1 - x0) / (y1 - y0)
            if px < xint:
                hit = not hit
    return hit


def main():
    if len(sys.argv) < 3:
        raise SystemExit(__doc__)
    svg = open(sys.argv[1]).read()
    out = sys.argv[2]
    scale = int(sys.argv[3]) if len(sys.argv) > 3 else 5

    m = re.search(r'viewBox="0 0 (\d+) (\d+)"', svg)
    vw, vh = (int(m.group(1)), int(m.group(2))) if m else (92, 92)

    # Work out the artwork's own resolution rather than assuming it. s-cam.svg is drawn on
    # a 4px grid, so sampling every 4th pixel reproduces it exactly and cheaply; a file drawn
    # at full resolution has a pitch of 1 and must be sampled at every pixel. Assuming 4 threw
    # away three quarters of a 1px-pitch drawing and silently produced a blurred-looking
    # fraction of the art, which is not something the output makes obvious.
    polys = paths(svg)
    coords = {int(v) for p in polys for pt in p for v in pt if float(v).is_integer()}
    cell = reduce(gcd, sorted(coords - {0})) if len(coords - {0}) > 1 else 1
    while (vw % cell) or (vh % cell):
        cell -= 1
    gw, gh = vw // cell, vh // cell

    # sample each cell centre - exact for grid-aligned art, no antialiasing
    art = [[any(inside(gx * cell + cell / 2, gy * cell + cell / 2, p) for p in polys)
            for gx in range(gw)] for gy in range(gh)]

    if "--preview" in sys.argv:
        for row in art:
            print("".join("#" if c else "." for c in row))

    sw, sh = gw * scale, gh * scale
    if sw > FB or sh > FB:
        raise SystemExit(f"scale {scale} gives {sw}x{sh}, larger than the {FB}x{FB} panel")
    ox, oy = (FB - sw) // 2, (FB - sh) // 2

    # False (black) everywhere the artwork does not reach
    px = [[False] * FB for _ in range(FB)]
    for y in range(sh):
        for x in range(sw):
            px[oy + y][ox + x] = art[y // scale][x // scale]

    packed, cur, cnt = [], 0, 0
    for y in range(FB):
        for x in reversed(range(FB)):        # flip left-right
            cur |= (0 if px[y][x] else 1) << (31 - cnt)   # 1 = black
            cnt += 1
            if cnt == 32:
                packed.append(cur)
                cur, cnt = 0, 0

    with open(out, "w") as f:
        f.write("#![cfg_attr(rustfmt, rustfmt_skip)]\n")
        # Record the source relative to the repo root. An absolute argv path bakes in one
        # machine's layout, and this repo has already outlived one such path.
        src = os.path.relpath(os.path.abspath(sys.argv[1]),
                              os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", ".."))
        f.write(f"// Generated by svgtorust.py from {src} at {scale}x "
                f"({gw}x{gh} art -> {sw}x{sh} centred in {FB}x{FB}).\n")
        f.write("pub const BITMAP: [u32; 512] = [\n")
        for i in range(512 // 4):
            f.write("  0x{:08x}, 0x{:08x}, 0x{:08x}, 0x{:08x},\n".format(
                packed[i * 4 + 3], packed[i * 4 + 2], packed[i * 4 + 1], packed[i * 4 + 0]))
        f.write("\n];\n")
    on = sum(r.count(True) for r in art)
    print(f"{out}: {gw}x{gh} art, {on} cells set, scaled {scale}x to {sw}x{sh}, centred in {FB}x{FB}")


if __name__ == "__main__":
    main()
