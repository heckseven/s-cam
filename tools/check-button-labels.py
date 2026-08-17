#!/usr/bin/env python3
"""Check that every button bar fits on the panel.

The bar names all three buttons on one 128px line. When a label does not fit its box the
graphics server drops it outright rather than clipping it, so the symptom is a button with no
label at all - invisible in review, and on hardware it reads as a rendering fault rather than
a layout one. That has now been found twice, once for the RIGHT label specifically.

The drawing code sizes each box to its own text and shortens the longest label if the three
cannot fit, so nothing disappears any more. This checks the other half: that no screen is
quietly relying on that shortening, because a truncated "sel..." is still a worse label than
one that was chosen to fit.

Capacity is deliberately pessimistic. The panel is 128px and the font's tables claim 7px per
glyph, but the server lays out fewer than the tables promise - which is the whole reason this
file exists - so budget 8px, giving 16 cells.

Run from the dc34-vault checkout: python3 tools/check-button-labels.py
"""

import re
import sys

SOURCES = [
    "src/ux.rs",
    "src/theme.rs",
    # the modals draw this bar too, from the shared implementation
    "../xous-core/libs/ux-api/src/widgets/radiobuttons.rs",
    "../xous-core/libs/ux-api/src/widgets/checkboxes.rs",
]
CELL_PX = 8      # pessimistic: the server lays out fewer cells than 7px/glyph implies
PANEL_PX = 128
PAD_PX = 4       # slack the drawing code puts around each label
CAPACITY = (PANEL_PX - PAD_PX * 3) // CELL_PX

# Every label is four characters. That is not a style rule, it is what makes the bar fit:
# three four-character labels plus their slack is the widest arrangement the panel holds, and
# every longer word tried so far has had a four-character synonym already in use elsewhere
# (select -> pick, retry -> redo). Keeping them uniform also means no label is ever the one
# that gets shortened.
MAX_LABEL = 4


def call_args(src, start):
    i = src.index("(", start)
    depth = 0
    for j in range(i, len(src)):
        if src[j] == "(":
            depth += 1
        elif src[j] == ")":
            depth -= 1
            if depth == 0:
                return src[i + 1 : j]
    return ""


def main():
    problems, checked = [], 0
    for path in SOURCES:
        try:
            src = open(path).read()
        except FileNotFoundError:
            continue
        for m in re.finditer(r"button_labels\s*\(", src):
            args = call_args(src, m.start())
            line = src.count("\n", 0, m.start()) + 1
            # the label slots are the string literals; None slots contribute nothing
            labels = re.findall(r'Some\(\s*"([^"]*)"\s*\)', args)
            if not labels:
                continue
            checked += 1
            for t in labels:
                if len(t) > MAX_LABEL:
                    problems.append(
                        f'{path}:{line}: label "{t}" is {len(t)} characters; the bar is sized '
                        f"for {MAX_LABEL}"
                    )
            cells = sum(len(t) for t in labels) + max(0, len(labels) - 1)
            if cells > CAPACITY:
                problems.append(
                    f"{path}:{line}: labels {labels} need {cells} cells, panel holds "
                    f"{CAPACITY} - the longest will be shortened at runtime"
                )

    if problems:
        print("FAIL")
        for p in problems:
            print("  " + p)
        sys.exit(1)
    print(f"OK: {checked} button bar(s) fit in {CAPACITY} cells")


if __name__ == "__main__":
    main()
