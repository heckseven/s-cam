#!/usr/bin/env python3
"""Check that every camera invocation goes through the shared camera routine.

`ActionOp::AcquireQr` is a blocking call: the app is stopped inside it for the whole scan and
cannot watch the buttons, so the value it returns is the only way it learns what happened -
including that the user asked to keep the frame. A call site that sends the opcode itself and
drops the result silently discards every outcome the camera reports.

That is not hypothetical. There were two copies of the camera sequence, photo capture was
wired into one of them, and the feature was dead for anyone opening the camera the usual way
while the code read as correct. Both now call `run_camera_scan()`.

Run from anywhere; exits non-zero if any call site bypasses it.
"""

import pathlib
import re
import sys

MAIN = pathlib.Path(__file__).resolve().parent.parent / "src" / "main.rs"
OPCODE = "ActionOp::AcquireQr"
ROUTINE = "run_camera_scan"


def strip_noise(src):
    """Blank out comments and string literals, preserving offsets and line numbers.

    Matching is done on the blanked copy so prose can neither be mistaken for a call site nor
    satisfy the check on behalf of one. A comment reading "TODO: call run_camera_scan()" used
    to be enough to make a broken site pass, which is the exact failure this guards against.

    Characters are replaced rather than removed, so offsets still index the original text.
    """
    out = list(src)
    i, n = 0, len(src)
    while i < n:
        two = src[i : i + 2]
        if two == "//":
            j = src.find("\n", i)
            j = n if j < 0 else j
            for k in range(i, j):
                out[k] = " "
            i = j
        elif two == "/*":
            j = src.find("*/", i + 2)
            j = n if j < 0 else j + 2
            for k in range(i, j):
                if out[k] != "\n":
                    out[k] = " "
            i = j
        elif src[i] == '"':
            j = i + 1
            while j < n:
                if src[j] == "\\":
                    j += 2
                    continue
                if src[j] == '"':
                    j += 1
                    break
                j += 1
            for k in range(i, min(j, n)):
                if out[k] != "\n":
                    out[k] = " "
            i = j
        else:
            i += 1
    return "".join(out)


def enclosing_arm(src, pos):
    """Return the innermost `{ ... }` block containing `pos`.

    Anchoring to the real block beats a fixed lookahead window: a window long enough to span
    one call site can reach into the next, letting a broken site pass on its neighbour's
    correctness, while a shorter one breaks as soon as a log line is added between the send
    and the handler. check-menu-wiring.py matches braces for the same reason.
    """
    depth, start = 0, None
    for i in range(pos, -1, -1):
        if src[i] == "}":
            depth += 1
        elif src[i] == "{":
            if depth == 0:
                start = i
                break
            depth -= 1
    if start is None:
        return src

    depth = 0
    for j in range(start, len(src)):
        if src[j] == "{":
            depth += 1
        elif src[j] == "}":
            depth -= 1
            if depth == 0:
                return src[start : j + 1]
    return src[start:]


def main():
    try:
        text = MAIN.read_text()
    except OSError as e:
        print(f"FAIL: cannot read {MAIN}: {e}")
        return 1

    code = strip_noise(text)

    if f"fn {ROUTINE}(" not in code:
        print(f"FAIL: {ROUTINE}() is missing; nothing owns the camera sequence")
        return 1

    sites = list(re.finditer(re.escape(OPCODE), code))
    if not sites:
        print(f"FAIL: no {OPCODE} call sites found - has the opcode been renamed?")
        return 1

    # The body of run_camera_scan is the one place allowed to send the opcode. Locate it by
    # span rather than by looking for the function name near the call: the name sits before
    # the opening brace, so it is not inside the block the call actually lives in.
    definition = (0, 0)
    d = code.find(f"fn {ROUTINE}(")
    if d >= 0:
        brace = code.find("{", d)
        if brace >= 0:
            body = enclosing_arm(code, brace + 1)
            definition = (brace, brace + len(body))

    problems = []
    for m in sites:
        line = text[: m.start()].count("\n") + 1
        if definition[0] <= m.start() < definition[1]:
            continue
        arm = enclosing_arm(code, m.start())
        if f"{ROUTINE}(" in arm:
            continue
        problems.append(
            f"main.rs:{line} sends {OPCODE} without going through {ROUTINE}() - "
            f"everything the camera reports is discarded here"
        )

    for p in problems:
        print(f"FAIL: {p}")
    if problems:
        return 1
    print(f"OK: all {len(sites)} camera call site(s) go through {ROUTINE}()")
    return 0


if __name__ == "__main__":
    sys.exit(main())
