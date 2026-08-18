#!/usr/bin/env python3
"""Check that menu_just_opened is only armed where a stale MenuClosed is actually coming.

The menu widget sends MenuClosed to its parent in two cases: when an entry that closes on
select is chosen, and when LEFT backs out. So opening menu B from an entry in menu A produces
a MenuClosed from A that must be ignored, or the app would paint itself over the menu it just
raised. `menu_just_opened` is what ignores it.

But a menu opened by a BUTTON ON A SCREEN has no menu open to send that notice. Arming the
flag there left it armed until the user pressed LEFT - and their own press was then swallowed
as if it were the stale one, so the menu sat there doing nothing. A second press worked, which
on hardware read as the screen being slow to respond rather than as a bug.

The rule: arm it only inside a handler for an opcode that a menu entry actually dispatches.
The entry tables in idlemenu.rs are the authority on which those are.

Run from the dc34-vault checkout: python3 tools/check-menu-just-opened.py
"""

import re
import sys

MAIN = "src/main.rs"
MENUS = "src/idlemenu.rs"


def menu_dispatched_ops():
    """Every VaultOp named in an entry table in idlemenu.rs."""
    src = open(MENUS).read()
    # Entries look like ("label", VaultOp::Foo) or ("label", VaultOp::Foo, 0).
    return set(re.findall(r'\(\s*"[^"]*"\s*,\s*VaultOp::(\w+)', src))


def main():
    dispatched = menu_dispatched_ops()
    if not dispatched:
        print(f"FAIL\n  {MENUS}: no menu entries parsed - the format changed")
        sys.exit(1)

    lines = open(MAIN).read().split("\n")
    problems, checked = [], 0
    for i, line in enumerate(lines):
        if "menu_just_opened = true;" not in line or line.lstrip().startswith("//"):
            continue
        checked += 1
        # Walk back to the enclosing match arm.
        op = None
        for j in range(i, max(-1, i - 20), -1):
            m = re.search(r"Some\(VaultOp::(\w+)\)\s*=>", lines[j])
            if m:
                op = m.group(1)
                break
        if op is None:
            problems.append(
                f"{MAIN}:{i + 1}: arms menu_just_opened outside any VaultOp handler, so it is "
                f"reached from a key press and no stale MenuClosed is coming"
            )
        elif op not in dispatched:
            problems.append(
                f"{MAIN}:{i + 1}: arms menu_just_opened in VaultOp::{op}, which no menu entry "
                f"dispatches - it is sent from a screen, so the flag will eat the user's own "
                f"LEFT press instead of a stale close notice"
            )

    if checked == 0:
        print(f"FAIL\n  {MAIN}: no menu_just_opened assignments found - the parse is wrong")
        sys.exit(1)
    if problems:
        print("FAIL")
        for p in problems:
            print("  " + p)
        sys.exit(1)
    print(f"OK: all {checked} menu_just_opened site(s) follow a menu-dispatched opcode")


if __name__ == "__main__":
    main()
