#!/usr/bin/env python3
"""Check that a screen handling the middle button also swallows it.

handle_key returns the key to the main loop when a screen does not consume it, and the main
loop reads a stray middle button as "open the camera". So a screen that acts on the middle
button and then falls through to `Some(k)` does both things at once: its own action, and the
camera on top of it.

That is what happened to the QR actions menu. It only showed up there because the password
and 2fa screens have the same fault and no records to test them with, so nobody could reach
it. This checks all of them rather than the one that was noticed.

Run from the s-cam checkout: python3 tools/check-key-consumption.py
"""

import re
import sys

UX = "src/ux.rs"
FIRE = "'\U0001f525'"


def main():
    src = open(UX).read()
    try:
        start = src.index("pub(crate) fn handle_key")
    except ValueError:
        print(f"FAIL\n  {UX}: no handle_key found - has it been renamed?")
        sys.exit(1)

    arms = re.split(r"\n            (?=VaultMode::)", src[start:])
    problems, checked = [], 0
    for arm in arms[1:]:
        name = arm.split("=>")[0].strip().replace("VaultMode::", "").split()[0]
        if FIRE not in arm:
            continue
        checked += 1

        # what the arm evaluates to when it runs off the end
        tail = [l.strip() for l in arm.splitlines() if l.strip()]
        falls_through = "Some(k)" in tail[-6:]
        if not falls_through:
            continue

        # ...then the middle-button branch has to bail out itself
        after = arm.split(FIRE, 1)[1][:900]
        if "return None" not in after:
            problems.append(
                f"{UX}: VaultMode::{name} acts on the middle button but falls through to "
                f"Some(k), so the main loop will open the camera on top of whatever it did"
            )

    if problems:
        print("FAIL")
        for p in problems:
            print("  " + p)
        sys.exit(1)
    print(f"OK: {checked} screen(s) handle the middle button without leaking it")


if __name__ == "__main__":
    main()
