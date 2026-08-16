#!/usr/bin/env python3
"""Check that every idle-menu entry reaches a handler that can actually receive it.

The menu widget in ux-api sends its actions as NON-blocking scalars. A handler written with
`msg_blocking_scalar_unpack!` never matches one, so the menu item silently does nothing -
no error, no log, just a dead entry. That is exactly how "Scan URL" shipped broken.

An entry can also be dead by pointing at an opcode `main.rs` does not handle at all, or by
two entries sharing one opcode (which is how "settings" swallowed the LED and screen-off
routes: three settings, one destination).

Finally, the widget closes on select but sets a `menu_active` flag that only `MenuDone`
used to clear. A handler that does not clear it leaves every later key routed back into
the closed menu, so the screen it just opened never receives one - which killed the BACK
button on every submenu at once.

Run from anywhere; exits non-zero on any dead entry.
"""

import pathlib
import re
import sys

SRC = pathlib.Path(__file__).resolve().parent.parent / "src"


def menu_entries(text):
    """Yield (source, label, opcode) for every entry across the whole menu tree.

    Entries live in two shapes now: inside a `create_*` builder, and in `const` tables used to
    fill the one reusable menu. Both must be walked, or a whole screen's worth of entries goes
    unchecked - which is how the settings entries became unreachable in the first place.
    """
    out = []

    for name in re.findall(r"pub fn create_(\w+)\(", text):
        body = re.search(r"pub fn create_%s\(.*?\n\}" % name, text, re.S).group(0)
        for label, opcode in re.findall(r'\("([^"]+)",\s*VaultOp::(\w+)[,)]', body):
            out.append((f"create_{name}", label, opcode))

    for name, body in re.findall(r"pub const (\w+): \[\(&str, VaultOp, u32\); \d+\] =(.*?)\];", text, re.S):
        for label, opcode in re.findall(r'\("([^"]+)",\s*VaultOp::(\w+)', body):
            out.append((name, label, opcode))

    if not out:
        sys.exit("found no menu entries at all in idlemenu.rs - has the shape changed?")
    return out


def handler_bodies(text):
    """Map opcode name -> handler body, for `Some(VaultOp::X) => { ... }` arms in main.rs.

    Arms are found by brace matching rather than regex: the bodies nest freely, and an
    arm-stripping regex has previously eaten code it should not have.

    The captured text starts at `=>`, not at the opening brace, because the thing being
    looked for can sit between the two: `=> msg_blocking_scalar_unpack!(msg, .., {`. An
    earlier version of this check started at the brace and so missed precisely the bug it
    exists to catch.
    """
    bodies = {}
    for m in re.finditer(r"Some\(VaultOp::(\w+)\)\s*=>", text):
        rest = text[m.end():]
        start = rest.find("{")
        if start < 0:
            continue
        depth, i = 0, start
        while i < len(rest):
            if rest[i] == "{":
                depth += 1
            elif rest[i] == "}":
                depth -= 1
                if depth == 0:
                    break
            i += 1
        bodies[m.group(1)] = rest[:i]
    return bodies


def main():
    entries = menu_entries((SRC / "idlemenu.rs").read_text())
    bodies = handler_bodies((SRC / "main.rs").read_text())
    problems = []

    # Opcodes that move between menus rather than opening a screen. They deliberately keep the
    # menu tree open, so they must NOT clear the active menu the way screen handlers do.
    #
    # The three photo actions are here because each one asks for confirmation first, which is
    # itself a menu - so their handlers open the confirm menu rather than closing anything.
    # The confirm menu's own answers do close it, and are checked normally.
    navigation = {
        "MenuRoot",
        "MenuLogin",
        "MenuSettings",
        "MenuDone",
        "PhotoExportB64",
        "PhotoExportAscii",
        "PhotoDelete",
    }

    seen = {}
    for menu, label, opcode in entries:
        key = (menu, opcode)
        if key in seen:
            problems.append(
                f'{menu}: "{label}" and "{seen[key]}" both send VaultOp::{opcode}, '
                f"so one of them is unreachable"
            )
        seen[key] = label

        body = bodies.get(opcode)
        if body is None:
            problems.append(
                f'{menu}: "{label}" sends VaultOp::{opcode}, which main.rs does not handle'
            )
        elif "msg_blocking_scalar_unpack!" in body:
            problems.append(
                f'{menu}: "{label}" sends VaultOp::{opcode}, whose handler uses '
                f"msg_blocking_scalar_unpack! - menu scalars are non-blocking and will never match"
            )
        elif opcode not in navigation and "active_menu = ActiveMenu::None" not in body:
            problems.append(
                f'{menu}: "{label}" sends VaultOp::{opcode}, whose handler never closes '
                f"the menu - keys will keep going to it and the screen it opens will be inert"
            )

    for p in problems:
        print(f"FAIL: {p}")
    if problems:
        return 1
    print(f"OK: all {len(entries)} menu entries across {len(set(m for m, _, _ in entries))} menus are live")
    return 0


if __name__ == "__main__":
    sys.exit(main())
