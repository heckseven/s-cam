#!/usr/bin/env python3
"""Fail the build if unreferenced code creeps back into dc34-vault.

The compiler is the only reliable authority on what is dead here - grep cannot see through
`cfg`, macros or trait impls, and a private module of a binary crate cannot be reached from
outside, so `dead_code` over the bin target is exact. About 1100 lines of write-only screens,
a dropped gene-exchange protocol and four orphaned bitmaps were removed on that evidence.

What survives is listed below. Every entry is a FIELD on a type that is itself live, which is
a different thing from dead code:

  * the Bookmark fields are parsed straight out of the on-disk record format, so dropping them
    would change what the badge can read back;
  * the SanitizeError payloads carry the reason a URL was rejected;
  * FactoryTestState::Error and Passkey::key identify a failure and a credential.

None of them costs space in the binary. Removing information from an error or a record is a
regression, not a cleanup, so they are allowed rather than deleted.

Run from the s-cam checkout: python3 tools/check-dead-code.py
"""

import json
import subprocess
import sys

# (file, warning text) pairs that are expected. Keyed on the message rather than a line
# number so ordinary edits above them do not turn this into a nuisance.
ALLOWED = {
    ("src/sanitize.rs", "field `0` is never read"),
    ("src/sanitize.rs", "fields `len` and `cap` are never read"),
    ("src/storage.rs", "fields `key`, `label`, and `timestamp_unix` are never read"),
    ("src/storage.rs", "field `key` is never read"),
    ("src/ux.rs", "field `0` is never read"),
}

CMD = [
    "cargo", "build", "--release",
    "--target", "riscv32imac-unknown-xous-elf",
    "--features", "board-baosec",
    "--message-format=json",
]


def main():
    proc = subprocess.run(CMD, capture_output=True, text=True)
    if proc.returncode != 0:
        print("FAIL\n  cargo build failed; fix that before reading this check")
        print(proc.stderr[-2000:])
        sys.exit(1)

    unexpected = set()
    for line in proc.stdout.splitlines():
        try:
            rec = json.loads(line)
        except ValueError:
            continue
        if rec.get("reason") != "compiler-message":
            continue
        if rec.get("target", {}).get("name") != "dc34-vault":
            continue
        msg = rec["message"]
        if (msg.get("code") or {}).get("code") != "dead_code":
            continue
        for span in msg.get("spans", []):
            if not span.get("is_primary"):
                continue
            key = (span["file_name"], msg["message"])
            if key not in ALLOWED:
                unexpected.add((span["file_name"], span["line_start"], msg["message"]))

    if unexpected:
        print("FAIL")
        for f, ln, m in sorted(unexpected):
            print(f"  {f}:{ln}  {m}")
        print("\n  Remove it, or add it to ALLOWED with a reason if it is a field on a live type.")
        sys.exit(1)
    print(f"OK: no unreferenced code beyond the {len(ALLOWED)} allowed field warnings")


if __name__ == "__main__":
    main()
