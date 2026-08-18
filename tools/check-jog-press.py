#!/usr/bin/env python3
"""Check that a screen offering an actions menu handles the jog press too.

The main loop treats a bare '∴' arriving from handle_key as "open the record actions" -
new / edit / delete / filter, which are operations on a PASSWORD record. A screen that does
not handle the jog press itself falls through to that default, so pressing the jog wheel on
the QR collection offered to edit and delete a password from a list of URLs, and its "delete"
went to a different handler than the delete on the same screen's own actions menu.

The rule: a screen that acts on the middle button has an actions menu, so it must either
handle the jog press itself or swallow it. Only a screen that falls through to `Some(k)`
leaks it to the main loop, so that is what this looks for - a screen ending in `None` has
already absorbed the press and is fine.

Run from the dc34-vault checkout: python3 tools/check-jog-press.py
"""

import re
import sys

UX = "src/ux.rs"
FIRE = "'\U0001f525'"
JOG = "'∴'"


def main():
    src = open(UX).read()
    try:
        start = src.index("pub(crate) fn handle_key")
    except ValueError:
        print(f"FAIL\n  {UX}: no handle_key found - has it been renamed?")
        sys.exit(1)

    arms = re.split(r"\n            (?=VaultMode::)", src[start:])
    problems, checked = [], 0
    for raw in arms[1:]:
        name = raw.split("=>")[0].strip().replace("VaultMode::", "").split()[0]
        # Match against code only. The comments on these arms discuss the very characters
        # being looked for, so a raw text search reads an explanation of the bug as a fix
        # for it - which is exactly how the seeded-fault test caught this checker out.
        arm = "\n".join(l for l in raw.splitlines() if not l.lstrip().startswith("//"))
        # Idle is the one screen where the main loop's own handling is the correct answer:
        # there is no record under a cursor, and the jog press opens the top-level menu.
        if name == "Idle" or FIRE not in arm:
            continue
        checked += 1
        if JOG in arm:
            continue
        # An arm that runs off the end into `None` has already swallowed the press. Only one
        # ending in `Some(k)` hands it to the main loop.
        tail = [l.strip() for l in arm.splitlines() if l.strip()]
        if "Some(k)" not in tail[-6:]:
            continue
        problems.append(
            f"{name}: acts on the middle button but leaks the jog press to the main loop, "
            f"which answers '∴' by opening the record actions menu"
        )

    if checked == 0:
        print(f"FAIL\n  {UX}: no screen handles the middle button - the parse is wrong")
        sys.exit(1)

    if problems:
        print("FAIL")
        for p in problems:
            print(f"  {p}")
        sys.exit(1)
    print(f"OK: {checked} screen(s) with an actions menu also handle the jog press")


if __name__ == "__main__":
    main()
