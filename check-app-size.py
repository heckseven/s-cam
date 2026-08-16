#!/usr/bin/env python3
"""Fail the build if dc34-vault grows past the page count the badge loader can handle.

Background
----------
dc34-vault is the only process loaded via `--inis`, i.e. demand-paged out of the
encrypted swap partition rather than resident in flash. Empirically, on DC34 badge
hardware the loader hangs partway through its (textless) boot progress bar when this
app occupies more than 307 pages. The failure is silent: no message reaches the
screen, the bar simply stops, and the badge must be reflashed to recover.

The trigger is size alone, not code. This was confirmed by building a version with no
feature changes at all -- only 768 bytes of inert padding -- which reproduced the hang
identically to a real feature addition of the same size.

Note that `panic = "abort"` is NOT a safe way to buy headroom: it shrinks the app a
long way (to ~259 pages) but changes codegen and section ordering, and the resulting
image also fails to boot. Reduce size via `opt-level` or by removing code.

Usage
-----
    ./check-app-size.py target/riscv32imac-unknown-xous-elf/release/dc34-vault

Exits non-zero if the limit is exceeded.
"""

import struct
import sys

PAGE_SIZE = 0x1000
# Highest page count observed to boot on hardware. 308 pages hangs the loader.
# 307 is the measured ceiling. A 283-page build once failed to boot and 282 booted, which
# looked like the ceiling had moved - but padding the booting build to 283 with inert bytes
# booted fine, so size was not the cause. Do not lower this on the strength of a correlation
# without running that control: pad the last good build to the failing count and flash it.
#
# Size is only one way to stall the loader. Code layout is another - see DECISIONS.md on
# panic="abort", which failed to boot at a much SMALLER size purely by changing section order.
MAX_PAGES = 307


def top_vaddr(path):
    """Highest virtual address the image occupies, across all PT_LOAD segments."""
    with open(path, "rb") as fh:
        elf = fh.read()
    if elf[:4] != b"\x7fELF":
        raise SystemExit(f"{path}: not an ELF file")

    e_phoff = struct.unpack_from("<I", elf, 0x1C)[0]
    e_phentsize = struct.unpack_from("<H", elf, 0x2A)[0]
    e_phnum = struct.unpack_from("<H", elf, 0x2C)[0]

    top = 0
    for i in range(e_phnum):
        off = e_phoff + i * e_phentsize
        p_type, _, p_vaddr, _, _, p_memsz, _, _ = struct.unpack_from("<8I", elf, off)
        if p_type == 1:  # PT_LOAD
            top = max(top, p_vaddr + p_memsz)
    return top


def main():
    if len(sys.argv) != 2:
        raise SystemExit(f"usage: {sys.argv[0]} <path-to-dc34-vault-elf>")

    top = top_vaddr(sys.argv[1])
    pages = (top + PAGE_SIZE - 1) // PAGE_SIZE
    headroom = MAX_PAGES - pages

    print(f"dc34-vault top vaddr : {top:#08x}")
    print(f"pages                : {pages} (limit {MAX_PAGES})")

    if pages > MAX_PAGES:
        print(
            f"\nFAIL: {pages - MAX_PAGES} page(s) over the limit.\n"
            f"This image will hang the badge loader partway through the boot progress\n"
            f"bar, with no error on screen. Shrink the app before flashing -- try\n"
            f'opt-level = "z" in [profile.release]. Do not use panic = "abort".',
            file=sys.stderr,
        )
        return 1

    print(f"OK: {headroom} page(s) of headroom")
    return 0


if __name__ == "__main__":
    sys.exit(main())
