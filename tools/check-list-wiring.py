#!/usr/bin/env python3
"""Check the list-drawing call sites in ux.rs.

`theme::list()` grew two trailing arguments - a marquee tick and a repaint mode - and its
neighbour `theme::button_labels()` also ends in a run of `None`s. That makes the two easy to
confuse: a repaint argument appended to the wrong call still reads plausibly. It happened
once already while this was being written.

The partial repaint carries a second invariant no type can express: a screen that paints only
its focused row must have painted the rest of itself first. `list_quantum == 0` is what says
"paint all of it", so every path that arrives at the bookmark list has to reset it. Miss one
and the list paints a single row over whatever screen came before.

Run from the dc34-vault checkout: python3 tools/check-list-wiring.py
"""

import re
import sys

UX = "src/ux.rs"
MAIN = "src/main.rs"


def call_text(src, start):
    """Return the source of the call whose opening paren follows `start`."""
    i = src.index("(", start)
    depth = 0
    for j in range(i, len(src)):
        if src[j] == "(":
            depth += 1
        elif src[j] == ")":
            depth -= 1
            if depth == 0:
                return src[start : j + 1]
    return src[start:]


def line_of(src, idx):
    return src.count("\n", 0, idx) + 1


def find_calls(src, needle):
    return [(line_of(src, m.start()), call_text(src, m.start()))
            for m in re.finditer(re.escape(needle), src)]


def main():
    problems = []
    ux = open(UX).read()
    main_rs = open(MAIN).read()

    # 1. every list() draws with an explicit repaint mode
    lists = find_calls(ux, "crate::theme::list(")
    if not lists:
        problems.append(f"{UX}: no theme::list() call sites found - has the name changed?")
    for line, text in lists:
        if "Repaint::" not in text:
            problems.append(
                f"{UX}:{line}: theme::list() has no Repaint argument, so it cannot say "
                f"whether it is painting the whole screen or one animating row"
            )

    # 2. ...and the button bar, which ends in a similar run of Nones, has none
    for line, text in find_calls(ux, "crate::theme::button_labels("):
        if "Repaint::" in text:
            problems.append(
                f"{UX}:{line}: theme::button_labels() was given a Repaint argument - it "
                f"belongs to the theme::list() call above it"
            )

    # 3. a row-only repaint only makes sense alongside the tick that drives it
    for line, text in lists:
        if "Repaint::FocusedRow" in text and "list_quantum" not in text:
            problems.append(
                f"{UX}:{line}: theme::list() repaints only the focused row but passes no "
                f"list_quantum, so nothing advances the marquee"
            )

    # 4. every route into the bookmark list must force a full repaint
    for path, src in ((UX, ux), (MAIN, main_rs)):
        for m in re.finditer(r"=\s*VaultMode::BookmarkList\s*;", src):
            line = line_of(src, m.start())
            window = src[max(0, m.start() - 400) : m.end() + 400]
            if "list_quantum = 0" not in window and "load_bookmarks()" not in window:
                problems.append(
                    f"{path}:{line}: enters BookmarkList without resetting list_quantum, so "
                    f"the list will paint one row over the previous screen"
                )

    if problems:
        print("FAIL")
        for p in problems:
            print("  " + p)
        sys.exit(1)

    print(f"OK: {len(lists)} list call site(s) painted with an explicit repaint mode")


if __name__ == "__main__":
    main()
