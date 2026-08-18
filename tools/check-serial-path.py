#!/usr/bin/env python3
"""Check that everything sending bytes over serial goes through VaultUi::serial_out.

There were two copies of the send loop: one behind the badge's own "export ascii" menu entry
and one behind the console's `photo ascii`. They looked interchangeable and were not. The
moment one of them learned something the other did not, the two diverged - CRLF conversion
went into serial_out, so an export started from the badge arrived as a staircase in the
terminal while the identical export asked for over the REPL came out square. The behaviour a
terminal needs is not obvious enough to reimplement correctly by eye.

So `serial_send` may only be called from serial_out. Everything else calls serial_out.

Run from the dc34-vault checkout: python3 tools/check-serial-path.py
"""

import re
import sys

UX = "src/ux.rs"
SENDER = "serial_out"


def enclosing_fn(lines, idx):
    """Name of the function containing line `idx`, by scanning back for a signature."""
    for j in range(idx, -1, -1):
        m = re.search(r"\bfn\s+(\w+)\s*\(", lines[j])
        if m:
            return m.group(1)
    return None


def main():
    lines = open(UX).read().split("\n")
    problems, found = [], 0
    for i, line in enumerate(lines):
        if "serial_send" not in line or line.lstrip().startswith("//"):
            continue
        found += 1
        owner = enclosing_fn(lines, i)
        if owner != SENDER:
            problems.append(
                f"{UX}:{i + 1}: {owner or '<unknown>'}() calls serial_send directly instead of "
                f"{SENDER}(), so it will not get the terminal handling that lives there"
            )

    if found == 0:
        print(f"FAIL\n  {UX}: no serial_send call sites at all - the parse is wrong")
        sys.exit(1)
    if problems:
        print("FAIL")
        for p in problems:
            print("  " + p)
        sys.exit(1)
    print(f"OK: all {found} serial_send call site(s) are inside {SENDER}()")


if __name__ == "__main__":
    main()
