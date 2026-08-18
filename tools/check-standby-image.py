#!/usr/bin/env python3
"""Check that the standby image is chosen in exactly one place.

Three things can be on the standby screen - the DEFCON logo, the S-CAM logo, or a photo the
user promoted to bling - and which one depends on a setting read from the PDDB at boot. Any
code that names one of those bitmaps directly has hardcoded an answer to a question it was
not asked.

That is what the boot flash was: the menu warm-up, which drags the UI code out of swap before
the first draw, restored the back buffer with the S-CAM logo whatever the user had chosen. A
badge set to the DEFCON image showed the splash, then a flash of S-CAM bling, then DEFCON.

So the two standby bitmaps may only be named inside VaultUi::standby_bitmap(). The boot
splash is a different image and is exempt - it is the same on every badge.

Run from the dc34-vault checkout: python3 tools/check-standby-image.py
"""

import re
import sys

SELECTOR = "standby_bitmap"
# Bitmaps whose choice depends on a setting. scam_splash is deliberately absent: it is the
# boot splash, not a standby image, and is not selectable.
GOVERNED = ("dc_logo", "scam_logo")


def selector_span(src):
    """Line range of VaultUi::standby_bitmap(), which is allowed to name the bitmaps."""
    lines = src.split("\n")
    start = None
    for i, line in enumerate(lines):
        if re.search(rf"fn {SELECTOR}\s*\(", line):
            start = i
            break
    if start is None:
        return None
    depth = 0
    for i in range(start, len(lines)):
        depth += lines[i].count("{") - lines[i].count("}")
        if depth == 0 and i > start:
            return (start, i)
    return (start, len(lines) - 1)


def main():
    problems = []
    ux = open("src/ux.rs").read()
    span = selector_span(ux)
    if span is None:
        print(f"FAIL\n  src/ux.rs: no fn {SELECTOR}() - has it been renamed or removed?")
        sys.exit(1)

    found = 0
    for path in ("src/ux.rs", "src/main.rs", "src/actions.rs", "src/theme.rs"):
        try:
            lines = open(path).read().split("\n")
        except FileNotFoundError:
            continue
        for i, line in enumerate(lines):
            if line.lstrip().startswith("//"):
                continue
            for name in GOVERNED:
                if f"{name}::BITMAP" not in line:
                    continue
                found += 1
                inside = path == "src/ux.rs" and span[0] <= i <= span[1]
                if not inside:
                    problems.append(
                        f"{path}:{i + 1}: names {name}::BITMAP directly instead of calling "
                        f"{SELECTOR}(), so it paints one image regardless of the user's setting"
                    )

    if found == 0:
        print("FAIL\n  no standby bitmap references found at all - the parse is wrong")
        sys.exit(1)
    if problems:
        print("FAIL")
        for p in problems:
            print("  " + p)
        sys.exit(1)
    print(f"OK: all {found} standby bitmap reference(s) live in {SELECTOR}()")


if __name__ == "__main__":
    main()
