#!/usr/bin/env python3
"""Check that dismissing a menu also forgets what is believed to be on the panel.

Two screens paint only what changed since last time - the standby image, and a list
mid-marquee, which repaints its focused row and nothing else. Neither of them knows a menu
was drawn over the top. Coming back from the QR actions menu, the list repainted one row onto
the menu's leftovers: a single readable entry surrounded by the menu that was supposed to be
gone, which reads as the screen having half-changed.

So every site that clears `active_menu` has to call `vault_ui.invalidate()`. It is applied
uniformly rather than only where a list is involved, because the cost of a needless full
repaint is one frame and the cost of a missed one is a visibly broken screen.

Run from the dc34-vault checkout: python3 tools/check-menu-invalidate.py
"""

import re
import sys

MAIN = "src/main.rs"
DISMISS = re.compile(r"^\s*active_menu = ActiveMenu::None;\s*$")


def main():
    lines = open(MAIN).read().split("\n")
    problems, checked = [], 0
    for i, line in enumerate(lines):
        if not DISMISS.match(line) or "let mut" in line:
            continue
        checked += 1
        # The invalidation is emitted directly after the assignment. Allow a little slack so
        # a later edit that puts a comment between them does not fail the build.
        window = "\n".join(lines[i + 1 : i + 4])
        if "vault_ui.invalidate()" not in window:
            problems.append(
                f"{MAIN}:{i + 1}: dismisses a menu without calling vault_ui.invalidate(), so "
                f"a partially-painted screen can be left under it"
            )

    if checked == 0:
        print(f"FAIL\n  {MAIN}: found no menu dismissals - the parse is wrong")
        sys.exit(1)
    if problems:
        print("FAIL")
        for p in problems:
            print("  " + p)
        sys.exit(1)
    print(f"OK: all {checked} menu dismissal(s) invalidate the painted screen")


if __name__ == "__main__":
    main()
