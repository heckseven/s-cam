#!/usr/bin/env python3
"""Check that the photo buffer and the record of what it holds move together.

`pending_photo` holds photo bits from one of two places: the camera, or storage. Whether the
bits are the ones the cursor is pointing at cannot be answered from the buffer alone, so
`photo_loaded_key` records which stored photo is in there - or None when the bits came from
the camera.

Get them out of step and the badge exports the wrong photo. That is what it did: the reload
test was "is anything loaded?", which is true the moment one photo has been opened, so
selecting a second photo and exporting sent the first one's bits again. It looks like a
corrupted transfer rather than the wrong file, which is what made it hard to spot.

Run from the s-cam checkout: python3 tools/check-photo-buffer.py
"""

import sys

UX = "src/ux.rs"
WINDOW = 4  # lines within which the partner assignment must appear


def main():
    lines = open(UX).read().splitlines()
    problems = []
    assigns = 0
    for i, line in enumerate(lines):
        if "self.pending_photo =" not in line or line.strip().startswith("//"):
            continue
        assigns += 1
        near = "\n".join(lines[i : i + WINDOW])
        if "photo_loaded_key" not in near:
            problems.append(
                f"{UX}:{i + 1}: sets pending_photo without saying which photo it now holds "
                f"- set photo_loaded_key beside it (None if the bits came from the camera): "
                f"{line.strip()[:60]}"
            )

    if problems:
        print("FAIL")
        for p in problems:
            print("  " + p)
        sys.exit(1)
    print(f"OK: {assigns} photo buffer assignment(s) keep their key in step")


if __name__ == "__main__":
    main()
