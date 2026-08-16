#!/usr/bin/env python3
"""Check and view what the badge typed.

Feed it whatever landed in your editor, as a file or on stdin:

    python3 tools/show-export.py exported.txt
    pbpaste | python3 tools/show-export.py

It accepts either export format and tells you whether the transfer survived:

  * base64 - decodes the data URI, validates the BMP, reports anything that could
    not have come out of a base64 encoder, and draws the picture.
  * ascii art - reads the badge's four characters back and draws the picture.

Both are drawn with half-block characters, which the badge itself cannot type: the
HID keycode map only covers ASCII and silently drops anything else, so the badge
sends ASCII and the block rendering happens here.
"""

import sys

B64 = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/="
PREFIX = "data:image/bmp;base64,"
W = 128
# the badge's art alphabet: none, top, bottom, both
ART = {" ": (0, 0), '"': (1, 0), ".": (0, 1), "#": (1, 1)}


def draw(rows):
    """Print a bitmap as half-blocks: two pixel rows per character row."""
    out = []
    for y in range(0, len(rows), 2):
        top, bot = rows[y], rows[y + 1] if y + 1 < len(rows) else [0] * W
        line = "".join(
            {(0, 0): " ", (1, 0): "▀", (0, 1): "▄", (1, 1): "█"}[(t, b)]
            for t, b in zip(top, bot)
        )
        out.append(line.rstrip())
    print("\n".join(out))


def from_base64(text):
    body = text.split(PREFIX, 1)[1] if PREFIX in text else text
    body = "".join(body.split())

    bad = sorted({c for c in body if c not in B64})
    if bad:
        print(f"  {len(bad)} character(s) impossible in base64: {''.join(bad)}")
        print("  That is a transport fault, not an encoding one - the badge cannot emit these.")
    print(f"  received {len(body)} characters (a full export is about 2838)")
    if len(body) < 2000:
        print("  short: the transfer was cut off or characters were dropped")

    import base64
    try:
        raw = base64.b64decode("".join(c for c in body if c in B64) + "===", validate=False)
    except Exception as e:
        print(f"  FAILED to decode: {e}")
        return None
    if raw[:2] != b"BM":
        print(f"  not a BMP: starts with {raw[:2]!r}, expected b'BM'")
        return None
    print(f"  decoded {len(raw)} bytes, BMP header present (expected 2110)")

    offset = int.from_bytes(raw[10:14], "little")
    width = int.from_bytes(raw[18:22], "little", signed=True)
    height = int.from_bytes(raw[22:26], "little", signed=True)
    depth = int.from_bytes(raw[28:30], "little")
    print(f"  {width}x{abs(height)}, {depth}bpp, top-down={height < 0}")

    stride = width // 8
    rows = []
    for y in range(abs(height)):
        start = offset + y * stride
        line = raw[start : start + stride]
        if len(line) < stride:
            print(f"  pixel data ends early at row {y} of {abs(height)}")
            break
        rows.append([(line[x // 8] >> (7 - x % 8)) & 1 for x in range(width)])
    return rows


def from_art(text):
    lines = [ln for ln in text.splitlines() if ln.strip(" ")]
    print(f"  {len(lines)} rows (a full export is 64)")
    unknown = sorted({c for ln in lines for c in ln} - set(ART))
    if unknown:
        print(f"  unexpected characters: {''.join(unknown)}")
    rows = []
    for ln in lines:
        top, bot = [], []
        for c in ln.ljust(W)[:W]:
            t, b = ART.get(c, (0, 0))
            top.append(t)
            bot.append(b)
        rows += [top, bot]
    return rows


def strip_logs(text):
    """Drop the badge's log lines.

    The export shares the CDC port with the log stream, so a capture normally has
    "INFO:module: ..." lines mixed in. They are not part of the picture and would
    otherwise be read as corruption.
    """
    keep, dropped = [], 0
    for line in text.splitlines():
        if line.startswith(("INFO:", "WARN:", "ERR:", "ERROR:", "DEBUG:", "TRACE:")):
            dropped += 1
            continue
        keep.append(line)
    if dropped:
        print(f"  ignored {dropped} log line(s) sharing the port")
    return "\n".join(keep)


def main():
    text = open(sys.argv[1]).read() if len(sys.argv) > 1 else sys.stdin.read()
    if not text.strip():
        sys.exit("nothing to read")
    text = strip_logs(text)
    if not text.strip():
        sys.exit("only log lines were captured - the export itself did not arrive")

    # Decide by shape, not by how clean the characters are: a corrupted base64 export is full
    # of characters base64 cannot contain, and judging by that alone sent it down the art path
    # where the real diagnosis - "these characters are impossible" - never got printed.
    body = text.split(PREFIX, 1)[1] if PREFIX in text else text
    art_like = body.count("\n") > 4 and sum(c in ART for c in body) > len(body) * 0.9
    if art_like:
        print("reading as ascii art:")
        rows = from_art(text)
    else:
        print("reading as base64:")
        rows = from_base64(text)

    if rows:
        print()
        draw(rows)


if __name__ == "__main__":
    main()
