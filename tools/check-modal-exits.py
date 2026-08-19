#!/usr/bin/env python3
"""Check that every interactive modal lets the user back out.

A modal takes the whole screen and the keyboard with it, so the LEFT button is the only way
to say "not this" - every other screen on the badge uses it for exactly that. Two of these
widgets used to answer LEFT with `// ignore these navigation keys`, which left the user with
a question they could close only by answering it.

That reads as harmless in a diff. It is the same shape as the About screen having no exit at
all, and the same shape it would be if someone tidied the arm away again, so it is checked
rather than remembered.

A widget passes if its key_action matches '←' and does something in that arm. Widgets that
are not questions - a notification dismissed by any key, a progress bar with no input - have
no arm to check and are skipped.

Run from the s-cam checkout: python3 tools/check-modal-exits.py
"""

import os
import re
import sys

WIDGETS = "../xous-core/libs/ux-api/src/widgets"

# Widgets that take no decision from the user, so there is nothing to decline.
SKIP = {"notification.rs", "payload.rs", "mod.rs", "modal.rs", "action.rs", "scroll.rs"}


def arm_body(src, idx):
    """Source of the match arm whose pattern starts at idx."""
    arrow = src.index("=>", idx)
    rest = src[arrow + 2 :].lstrip()
    if rest.startswith("{"):
        depth = 0
        for j, ch in enumerate(rest):
            if ch == "{":
                depth += 1
            elif ch == "}":
                depth -= 1
                if depth == 0:
                    return rest[1:j]
        return rest
    return rest.split("\n", 1)[0]


def is_effectively_empty(body):
    lines = []
    for raw in body.splitlines():
        line = raw.strip()
        if not line or line.startswith("//"):
            continue
        lines.append(line)
    return not lines


def main():
    if not os.path.isdir(WIDGETS):
        print(f"cannot find {WIDGETS} - run this from the s-cam checkout")
        sys.exit(2)

    problems, checked = [], 0
    for name in sorted(os.listdir(WIDGETS)):
        if not name.endswith(".rs") or name in SKIP:
            continue
        path = os.path.join(WIDGETS, name)
        src = open(path).read()
        if "fn key_action" not in src:
            continue

        # only the arms inside key_action
        start = src.index("fn key_action")
        region = src[start:]

        hits = [m.start() for m in re.finditer(r"'←'", region)]
        if not hits:
            problems.append(
                f"{name}: key_action never matches '←', so the LEFT button does nothing "
                f"and the user cannot decline"
            )
            continue

        checked += 1
        if all(is_effectively_empty(arm_body(region, h)) for h in hits):
            problems.append(
                f"{name}: key_action matches '←' but the arm does nothing - the user still "
                f"cannot back out of this modal"
            )

    if problems:
        print("FAIL")
        for p in problems:
            print("  " + p)
        sys.exit(1)
    print(f"OK: {checked} interactive modal(s) can be backed out of with LEFT")


if __name__ == "__main__":
    main()
