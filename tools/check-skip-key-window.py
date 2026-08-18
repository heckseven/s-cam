#!/usr/bin/env python3
"""Check that the swallow-one-key window is armed AFTER the camera runs, not before.

`skip_key_until` exists to drop the button press that ends a camera session, so that the
press does not also land on whatever screen the app returns to. run_camera_scan blocks for the
entire session - as long as the user takes to frame a shot - so arming the window before that
call is useless: the deadline has always expired by the time the press arrives.

That is not a theoretical failure. Armed beforehand, the press that took the photo fell
through to the preview screen, where RIGHT means save, so every photo went straight to
storage and the keep/retake/discard step was never offered. It looked like the badge was
saving pictures the instant they were taken.

Run from the dc34-vault checkout: python3 tools/check-skip-key-window.py
"""

import sys

MAIN = "src/main.rs"
ARM = "skip_key_until = Some("
CALL = "run_camera_scan("
WINDOW = 15


def main():
    lines = open(MAIN).read().split("\n")
    calls = [
        i for i, l in enumerate(lines)
        if CALL in l and not l.lstrip().startswith("//") and "fn run_camera_scan" not in l
    ]
    if not calls:
        print(f"FAIL\n  {MAIN}: no run_camera_scan call sites found - the parse is wrong")
        sys.exit(1)

    problems, checked = [], 0
    for c in calls:
        near = [
            i for i in range(max(0, c - WINDOW), min(len(lines), c + WINDOW))
            if ARM in lines[i] and not lines[i].lstrip().startswith("//")
        ]
        if not near:
            continue
        checked += 1
        if all(i < c for i in near):
            problems.append(
                f"{MAIN}:{near[0] + 1}: arms the skip-key window before the run_camera_scan() "
                f"at line {c + 1}. The camera blocks for the whole session, so the window "
                f"expires before the press that ends it arrives - and that press then lands "
                f"on the screen underneath."
            )

    if checked == 0:
        print(f"FAIL\n  {MAIN}: no camera call site arms the skip-key window at all")
        sys.exit(1)
    if problems:
        print("FAIL")
        for p in problems:
            print("  " + p)
        sys.exit(1)
    print(f"OK: all {checked} camera call site(s) arm the skip-key window after the scan")


if __name__ == "__main__":
    main()
