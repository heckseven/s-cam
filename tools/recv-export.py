#!/usr/bin/env python3
"""Receive a badge export over USB serial.

    python3 tools/recv-export.py            # watch every /dev/ttyACM*, save what arrives
    python3 tools/recv-export.py /dev/ttyACM1 -o photo.txt

Use this rather than `cat`. The port comes up with `min = 0; time = 0`, which makes
read() return zero bytes immediately - `cat` reads that as end-of-file and exits at
once, which looks exactly like the badge having sent nothing. This sets the line to
raw with a blocking read first.

It also watches every ACM port at once by default, because the badge can present
more than one and the export is not necessarily on the lowest-numbered.
"""

import argparse
import glob
import os
import select
import sys
import termios
import time


def open_raw(path):
    fd = os.open(path, os.O_RDONLY | os.O_NOCTTY | os.O_NONBLOCK)
    attrs = termios.tcgetattr(fd)
    iflag, oflag, cflag, lflag, ispeed, ospeed, cc = attrs
    # raw: no translation, no echo, no signal characters
    iflag &= ~(termios.IXON | termios.IXOFF | termios.ICRNL | termios.INLCR | termios.IGNCR)
    oflag &= ~termios.OPOST
    lflag &= ~(termios.ICANON | termios.ECHO | termios.ECHOE | termios.ISIG)
    # CLOCAL: do not wait on carrier. CREAD: actually receive.
    cflag |= termios.CLOCAL | termios.CREAD
    cc[termios.VMIN] = 1
    cc[termios.VTIME] = 0
    termios.tcsetattr(fd, termios.TCSANOW, [iflag, oflag, cflag, lflag, ispeed, ospeed, cc])
    return fd


def other_readers(paths):
    """Find other processes holding these ports open.

    A tty delivers each byte to exactly one reader. The badge's log capture listens on every
    interface, so leaving it running during an export silently splits the transfer between
    the two tools and the image arrives corrupt - which looks like a badge fault and is not.
    """
    import glob as _glob

    want = {os.path.realpath(p) for p in paths}
    found = []
    for fd_dir in _glob.glob("/proc/[0-9]*/fd"):
        pid = fd_dir.split("/")[2]
        if pid == str(os.getpid()):
            continue
        try:
            for entry in os.listdir(fd_dir):
                try:
                    target = os.readlink(os.path.join(fd_dir, entry))
                except OSError:
                    continue
                if target in want:
                    try:
                        name = open(f"/proc/{pid}/comm").read().strip()
                    except OSError:
                        name = "?"
                    found.append((int(pid), name, target))
                    break
        except OSError:
            continue
    return found


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("ports", nargs="*", help="serial devices; default is every /dev/ttyACM*")
    ap.add_argument("-o", "--out", default="export.txt")
    ap.add_argument("--idle", type=float, default=2.0, help="stop after this many quiet seconds")
    ap.add_argument("--wait", type=float, default=300.0,
                    help="give up if no export data arrives at all. Generous on purpose: "
                         "this is the time you have to walk the badge menus.")
    args = ap.parse_args()

    ports = args.ports or sorted(glob.glob("/dev/ttyACM*"))
    if not ports:
        sys.exit("no /dev/ttyACM* found - is the badge plugged in?")

    fds = {}
    for p in ports:
        try:
            fds[open_raw(p)] = p
        except OSError as e:
            print(f"  {p}: cannot open ({e})")
    if not fds:
        sys.exit("could not open any port")

    rivals = other_readers(fds.values())
    if rivals:
        print()
        print("  WARNING: these processes are also reading the same port(s):")
        for pid, name, path in rivals:
            print(f"    pid {pid} {name}  ->  {path}")
        print("  Two readers split the stream between them - each byte goes to exactly one.")
        print("  The export will arrive incomplete. Stop the other reader first:")
        print(f"    kill {' '.join(str(p) for p, _, _ in rivals)}")
        print()

    print(f"listening on {', '.join(fds.values())}")
    print("now run the export on the badge: photos -> more -> export -> yes")

    # The badge's log shares these interfaces with the export. Log traffic must not start
    # the idle countdown, or simply walking the menus to reach the export ends the capture
    # before it begins - which is exactly what it did.
    def is_log(line):
        return line.startswith(
            (b"INFO:", b"WARN:", b"ERR:", b"ERROR:", b"DEBUG:", b"TRACE:")
        ) or b"(src/" in line or b"(services/" in line

    data = bytearray()
    source = None
    started = time.time()
    last = None          # last time PAYLOAD arrived, not last time anything arrived
    # One partial-line buffer per port. A single shared one splices the tail of a line from
    # one interface onto the head of a line from another.
    pending = {fd: bytearray() for fd in fds}
    while True:
        ready, _, _ = select.select(list(fds), [], [], 0.25)
        for fd in ready:
            try:
                chunk = os.read(fd, 4096)
            except BlockingIOError:
                # select() said readable, but another reader took the bytes first. Harmless
                # on its own - the real problem is that it happened at all; see the warning
                # printed at startup.
                continue
            except OSError:
                continue
            if not chunk:
                continue
            pending[fd] += chunk
            # Judge whole lines only: a log line has to be complete to be recognised.
            parts = pending[fd].split(b"\n")
            pending[fd] = bytearray(parts.pop())  # trailing fragment, not yet a line
            for line in parts:
                stripped = line.rstrip(b"\r")
                # An empty line is never payload: an ASCII-art row is 128 characters (a blank
                # row is 128 spaces) and base64 lines are not empty either. Stray newlines do
                # arrive - one capture began with ten of them - and keeping them shifts every
                # row and makes the file look corrupt when the image is entirely intact.
                if not stripped:
                    continue
                if is_log(stripped):
                    continue
                if source is None:
                    source = fds[fd]
                    print(f"receiving on {source}")
                elif fds[fd] != source:
                    # The badge carries the same stream on more than one interface. Taking
                    # both concatenates the export with itself - chunks appear twice, and so
                    # does the trailing "==" of a base64 payload. Listen to one and ignore
                    # the rest.
                    continue
                data += line + b"\n"
                last = time.time()
        if last and time.time() - last > args.idle:
            break
        if not last and time.time() - started > args.wait:
            sys.exit("nothing but log traffic arrived - did the export run?")

    for fd in fds:
        os.close(fd)
    with open(args.out, "wb") as fh:
        fh.write(data)
    print(f"{len(data)} bytes from {source} -> {args.out}")
    print(f"now: python3 {os.path.dirname(__file__)}/show-export.py {args.out}")


if __name__ == "__main__":
    main()
